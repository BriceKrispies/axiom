//! The camera: a broadcast-arcade framing that is *derived* from the viewport,
//! not scaled down from a desktop one.
//!
//! The composition the game needs is fixed — ball low in frame, kicker beside
//! it, goal in the upper middle, and enough depth between them that a curve is
//! visibly a curve. The pose that produces it is fixed too: a shade above head
//! height, a little behind the ball, aimed at a point between the two.
//!
//! What is *not* fixed is the field of view, and that is the whole responsive
//! design. Rather than picking an angle and hoping, the camera is handed a list
//! of things that must be on screen — both goal corners, the top of the frame,
//! the ball, the kicker, and a strip of turf in front of the ball — and solves
//! for the narrowest vertical field of view that contains all of them at this
//! viewport's aspect. A 9:19.5 phone in portrait needs a tall frustum to fit the
//! goal's width; a 16:9 desktop needs a short one. Both fall out of the same
//! call, and neither is a special case.

use axiom::prelude::{Vec2, Vec3};

use crate::pitch::{GoalMouth, GOAL_HEIGHT, KEEPER_LINE_Z};
use crate::tuning::CameraTuning;

/// Where the camera is and what it is looking at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraPose {
    pub eye: Vec3,
    pub target: Vec3,
    pub fov_degrees: f32,
}

impl CameraPose {
    /// Blend two poses (the flight dolly rides on this).
    pub fn lerp(a: CameraPose, b: CameraPose, t: f32) -> CameraPose {
        let t = t.clamp(0.0, 1.0);
        CameraPose {
            eye: a.eye.add(b.eye.subtract(a.eye).mul_scalar(t)),
            target: a.target.add(b.target.subtract(a.target).mul_scalar(t)),
            fov_degrees: a.fov_degrees + (b.fov_degrees - a.fov_degrees) * t,
        }
    }
}

/// The camera's own basis: forward, right and up, from an eye and a target.
///
/// Right is `forward × up`, matching the engine's right-handed view transform.
/// With the eye behind the ball looking down `-Z` that resolves to world `+X`,
/// which is why "bend right" and "screen right" are the same direction and no
/// sign has to be remembered anywhere else in the game.
pub fn basis(eye: Vec3, target: Vec3) -> (Vec3, Vec3, Vec3) {
    let forward = target
        .subtract(eye)
        .normalize()
        .unwrap_or(Vec3::new(0.0, 0.0, -1.0));
    let right = forward
        .cross(Vec3::UNIT_Y)
        .normalize()
        .unwrap_or(Vec3::UNIT_X);
    (forward, right, right.cross(forward))
}

/// The narrowest vertical field of view (degrees) that keeps every point in
/// `must_see` inside the frame at this aspect.
pub fn fit_fov(
    eye: Vec3,
    target: Vec3,
    aspect: f32,
    must_see: &[Vec3],
    tuning: &CameraTuning,
) -> f32 {
    let (forward, right, up) = basis(eye, target);
    let aspect = aspect.max(0.05);
    let needed = must_see
        .iter()
        .map(|p| {
            let d = p.subtract(eye);
            let depth = d.dot(forward).max(0.35);
            let vertical = d.dot(up).abs() / depth;
            // A point that must fit horizontally costs vertical field of view in
            // proportion to how narrow the viewport is — which is exactly why a
            // portrait phone ends up with a tall frustum without anyone deciding
            // that it should.
            let horizontal = (d.dot(right).abs() / depth) / aspect;
            vertical.max(horizontal)
        })
        .fold(0.0f32, f32::max);
    (2.0 * (needed * tuning.fit_padding).atan()).to_degrees()
        .clamp(tuning.min_fov, tuning.max_fov)
}

/// The points the framing is not allowed to lose. Everything the player has to
/// read is in here, and nothing decorative is.
pub fn must_see(mouth: &GoalMouth, ball: Vec3, kicker: Vec3, tuning: &CameraTuning) -> Vec<Vec3> {
    let mut points: Vec<Vec3> = mouth.frame_corners().to_vec();
    // A little air above the crossbar, so a lob has somewhere to be seen.
    points.push(Vec3::new(0.0, GOAL_HEIGHT + 0.85, 0.0));
    // The ball, and the near-field allowance around it: a strip of turf so it
    // never kisses the bottom edge, and a shoulder's width either side at
    // roughly where the kicker plants.
    points.push(ball);
    points.push(Vec3::new(ball.x, 0.0, ball.z + tuning.near_depth));
    [-1.0f32, 1.0].into_iter().for_each(|side| {
        points.push(Vec3::new(
            ball.x + side * tuning.near_margin,
            0.95,
            ball.z + tuning.near_depth * 0.55,
        ));
    });
    // The kicker at the top of the run-up, taken at chest height rather than at
    // the crown: he is the closest thing to the camera by a long way, so
    // demanding every hair of him is what blows the frustum open and shrinks the
    // goal to a stamp. Chest height keeps him unmistakably in shot and lets the
    // padding carry the rest.
    points.push(Vec3::new(kicker.x, 1.35, kicker.z));
    points
}

/// The framing from inside the goal: the keeper's own eyes.
///
/// The other half of the game gets a different camera because it is a different
/// job. Taking a penalty you are looking *at* a goal and drawing a line into it,
/// so the goal has to be framed. Keeping one you are looking *out* at a person
/// walking toward a ball, and the only thing worth seeing is them — which run-up
/// angle they have taken, how fast they are coming, which way their hips are
/// open. First person, because a keeper does not watch themselves dive; and
/// because it puts the player where the decision is, which is the whole reason
/// for the mode existing.
///
/// The eye rides the keeper's own head, so a dive genuinely throws the view
/// sideways and down. That is not a flourish: it is the honest consequence of
/// having committed, and it is what makes going early *feel* like the gamble it
/// is — you commit, the world tips, and you watch the ball from wherever you
/// have put yourself.
pub fn keeper_eye(_viewport: Vec2, hips: Vec3, lean: f32, ball: Vec3, tuning: &CameraTuning) -> CameraPose {
    // Head height above the hips, banked with the dive.
    let bank = lean.clamp(-1.0, 1.0);
    let eye = Vec3::new(
        hips.x + bank * tuning.keeper_head_swing,
        hips.y + tuning.keeper_head_rise,
        hips.z + 0.12,
    );
    // Watching the ball while it is in front — but only while it is in front. A
    // ball that has gone past is a ball in the net behind your own head, and a
    // camera that kept following it would spin the view through 180 degrees at
    // the exact moment the player is trying to read what just happened.
    let spot = Vec3::new(0.0, 0.6, KEEPER_LINE_Z + 6.0);
    let watching = [spot, Vec3::new(ball.x, ball.y + 0.25, ball.z)]
        [usize::from(ball.z > hips.z + 0.5)];
    CameraPose {
        eye,
        target: watching,
        // A person's field of view, not a fisheye. Dividing this by a portrait
        // aspect turned 88 degrees into 160 and bent the whole pitch round the
        // edges of the phone.
        fov_degrees: tuning.keeper_fov,
    }
}

/// The framing for this viewport.
///
/// `dolly` (`0..1`) creeps the eye in over the flight — a small push that keeps
/// the ball growing as it travels without ever re-framing the goal.
pub fn frame(
    viewport: Vec2,
    mouth: &GoalMouth,
    ball: Vec3,
    kicker: Vec3,
    dolly: f32,
    tuning: &CameraTuning,
) -> CameraPose {
    let aspect = (viewport.x / viewport.y.max(1.0)).max(0.05);
    let creep = tuning.flight_dolly * dolly.clamp(0.0, 1.0);
    // A wide screen has horizontal room to spare and no vertical room to waste,
    // so the eye comes *forward* on landscape: the same fitted frustum then puts
    // more goal on the screen instead of more empty pitch either side of it.
    let close_in = tuning.landscape_close * (aspect - 1.0).clamp(0.0, 1.0);
    let eye = Vec3::new(
        0.0,
        tuning.eye_height,
        ball.z + tuning.eye_back - close_in - creep,
    );
    let target = Vec3::new(0.0, tuning.look_height, tuning.look_depth);
    CameraPose {
        eye,
        target,
        fov_degrees: fit_fov(
            eye,
            target,
            aspect,
            &must_see(mouth, ball, kicker, tuning),
            tuning,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pitch::{ball_spot, GOAL_HALF_WIDTH};
    use crate::tuning::Tuning;

    /// Where the kicker waits at the top of the run-up.
    const KICKER: Vec3 = Vec3::new(-1.04, 0.0, 14.2);

    fn pose_for(w: f32, h: f32) -> CameraPose {
        let tuning = Tuning::DEFAULT;
        frame(
            Vec2::new(w, h),
            &GoalMouth::new(tuning.goal.inset),
            ball_spot(tuning.flight.ball_radius),
            KICKER,
            0.0,
            &tuning.camera,
        )
    }

    #[test]
    fn the_basis_puts_world_plus_x_on_the_right_of_the_screen() {
        let (forward, right, up) = basis(Vec3::new(0.0, 3.0, 17.0), Vec3::new(0.0, 1.3, 3.0));
        assert!(forward.z < 0.0, "it looks toward the goal");
        assert!((right.x - 1.0).abs() < 1.0e-4, "right is +X: {right:?}");
        assert!(up.y > 0.9);
        // A degenerate look direction still yields a usable basis.
        let (_, r, _) = basis(Vec3::ZERO, Vec3::ZERO);
        assert_eq!(r, Vec3::UNIT_X);
    }

    #[test]
    fn every_phone_shape_keeps_the_whole_goal_and_the_ball_on_screen() {
        // The representative portrait viewports, plus a desktop landscape.
        for (w, h) in [
            (320.0f32, 568.0f32),
            (360.0, 800.0),
            (390.0, 844.0),
            (412.0, 915.0),
            (1440.0, 900.0),
        ] {
            let pose = pose_for(w, h);
            let (forward, right, up) = basis(pose.eye, pose.target);
            let half_y = (pose.fov_degrees * 0.5).to_radians().tan();
            let half_x = half_y * (w / h);
            let tuning = Tuning::DEFAULT;
            let points = must_see(
                &GoalMouth::new(tuning.goal.inset),
                ball_spot(tuning.flight.ball_radius),
                KICKER,
                &tuning.camera,
            );
            points.iter().for_each(|p| {
                let d = p.subtract(pose.eye);
                let depth = d.dot(forward);
                assert!(depth > 0.0, "{w}x{h}: {p:?} is behind the camera");
                assert!(
                    d.dot(up).abs() / depth <= half_y + 1.0e-3,
                    "{w}x{h}: {p:?} is off the top or bottom"
                );
                assert!(
                    d.dot(right).abs() / depth <= half_x + 1.0e-3,
                    "{w}x{h}: {p:?} is off the side"
                );
            });
        }
    }

    #[test]
    fn the_goal_gets_more_of_a_portrait_screen_than_a_landscape_one() {
        // The composition the game optimises for is the phone held upright, and
        // it earns that: the same fitted camera puts a much larger share of the
        // frame's width on the goal in portrait than on a wide desktop window.
        let share = |w: f32, h: f32| {
            let pose = pose_for(w, h);
            let (forward, right, _) = basis(pose.eye, pose.target);
            let half_x = (pose.fov_degrees * 0.5).to_radians().tan() * (w / h);
            let d = Vec3::new(GOAL_HALF_WIDTH, 0.0, 0.0).subtract(pose.eye);
            (d.dot(right) / d.dot(forward)) / half_x
        };
        let portrait = share(390.0, 844.0);
        let landscape = share(1440.0, 900.0);
        assert!(portrait > 0.7, "the goal only spans {portrait} of the half-width");
        assert!(
            portrait > landscape * 1.5,
            "portrait {portrait} should dwarf landscape {landscape}"
        );
        // Both stay inside the authored bounds.
        let t = Tuning::DEFAULT.camera;
        [pose_for(390.0, 844.0), pose_for(1440.0, 900.0)]
            .iter()
            .for_each(|p| {
                assert!(p.fov_degrees >= t.min_fov && p.fov_degrees <= t.max_fov);
            });
        // A wide window brings the eye forward rather than leaving the goal in
        // the distance.
        assert!(pose_for(1440.0, 900.0).eye.z < pose_for(390.0, 844.0).eye.z);
    }

    #[test]
    fn the_ball_sits_low_and_the_goal_sits_high_in_the_frame() {
        let tuning = Tuning::DEFAULT;
        let pose = pose_for(390.0, 844.0);
        let (forward, _, up) = basis(pose.eye, pose.target);
        let half_y = (pose.fov_degrees * 0.5).to_radians().tan();
        let height_of = |p: Vec3| {
            let d = p.subtract(pose.eye);
            (d.dot(up) / d.dot(forward)) / half_y
        };
        let ball = height_of(ball_spot(tuning.flight.ball_radius));
        let goal_top = height_of(Vec3::new(0.0, GOAL_HEIGHT, 0.0));
        assert!(ball < -0.35, "the ball is in the lower part: {ball}");
        assert!(goal_top > 0.0, "the goal is in the upper part: {goal_top}");
        // The goal is not a postage stamp in the distance: its mouth spans a
        // healthy fraction of the frame's width.
        let half_x = half_y * (390.0 / 844.0);
        let post = Vec3::new(GOAL_HALF_WIDTH, 1.0, 0.0).subtract(pose.eye);
        let width = (post.dot(basis(pose.eye, pose.target).1) / post.dot(forward)) / half_x;
        assert!(width > 0.55, "the goal only spans {width} of the half-width");
    }

    #[test]
    fn the_dolly_creeps_the_eye_in_without_re_framing() {
        let tuning = Tuning::DEFAULT;
        let still = pose_for(390.0, 844.0);
        let pushed = frame(
            Vec2::new(390.0, 844.0),
            &GoalMouth::new(tuning.goal.inset),
            ball_spot(tuning.flight.ball_radius),
            KICKER,
            1.0,
            &tuning.camera,
        );
        assert!(pushed.eye.z < still.eye.z);
        assert_eq!(pushed.target, still.target);
        assert_eq!(CameraPose::lerp(still, pushed, 0.0), still);
        assert_eq!(CameraPose::lerp(still, pushed, 1.0), pushed);
        assert!(CameraPose::lerp(still, pushed, 0.5).eye.z < still.eye.z);
    }
}
