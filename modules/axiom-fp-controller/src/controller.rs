//! The first-person walk+look controller facade.

use axiom_kernel::{Meters, Radians};
use axiom_math::{Mat4, Vec3};

use crate::ids::{Lens, LookDelta, MoveIntent, Pose, WalkTuning};

/// The engine's first-person walk+look controller: integrates a frame of held
/// input into a new [`Pose`], seats the eye above the terrain, and builds the
/// camera view-projection that pose implies. Zero-sized — a namespace for the
/// deterministic controller math, the standalone twin of the scene layer's
/// `ControllerSystem` for apps that drive a camera matrix directly (no scene
/// node). All three methods are pure functions of their arguments.
#[derive(Debug)]
pub struct FpController;

impl FpController {
    /// Integrate one frame of first-person movement + look into a new [`Pose`].
    ///
    /// `yaw` accumulates the key-turn (`turn_left − turn_right`, scaled by
    /// [`WalkTuning::turn_speed`]) plus the mouse-look yaw; `pitch` accumulates the
    /// mouse-look pitch, clamped to `±`[`WalkTuning::pitch_limit`]. The planar
    /// position steps along the forward axis (`forward − backward`) and the
    /// view-right axis (`strafe_right − strafe_left`), each scaled by
    /// [`WalkTuning::move_speed`] and rotated by the **new yaw only** — so looking
    /// up or down never tilts movement off the horizontal plane (`yaw 0` faces
    /// −Z, matching the perspective convention in [`Self::view_projection`]).
    pub fn step(pose: Pose, intent: MoveIntent, look: LookDelta, tuning: WalkTuning) -> Pose {
        let key_turn = (intent.turn_left as i32 - intent.turn_right as i32) as f32
            * tuning.turn_speed().get();
        let yaw = pose.yaw().get() + key_turn + look.yaw().get();
        let limit = tuning.pitch_limit().get();
        let pitch = (pose.pitch().get() + look.pitch().get()).clamp(-limit, limit);

        let speed = tuning.move_speed().get();
        let forward = (intent.forward as i32 - intent.backward as i32) as f32 * speed;
        let strafe = (intent.strafe_right as i32 - intent.strafe_left as i32) as f32 * speed;
        let (fx, fz) = (yaw.sin(), -yaw.cos());
        let x = pose.x().get() + fx * forward + fz * -strafe;
        let z = pose.z().get() + fz * forward - fx * -strafe;

        Pose::new(
            Meters::finite_or_zero(x),
            Meters::finite_or_zero(z),
            Radians::finite_or_zero(yaw),
            Radians::finite_or_zero(pitch),
        )
    }

    /// The world-space eye position for `pose`: the planar position seated
    /// [`WalkTuning::eye_height`] above `ground` (the terrain height sampled under
    /// the walker). This is the point [`Self::view_projection`] looks from, and
    /// the point a streaming world uses to plan residency around the camera.
    pub fn eye_position(pose: Pose, ground: Meters, tuning: WalkTuning) -> Vec3 {
        Vec3::new(pose.x().get(), ground.get() + tuning.eye_height().get(), pose.z().get())
    }

    /// The camera view-projection (`proj · view`) for `pose`, seated on `ground`,
    /// through `lens`. The eye looks along `(yaw, pitch)` with world +Y up; a
    /// degenerate lens or a coincident eye/target falls back to the identity so
    /// the matrix is always finite.
    pub fn view_projection(pose: Pose, ground: Meters, tuning: WalkTuning, lens: Lens) -> Mat4 {
        let eye = Self::eye_position(pose, ground, tuning);
        let (cp, sp) = (pose.pitch().get().cos(), pose.pitch().get().sin());
        let fwd = Vec3::new(pose.yaw().get().sin() * cp, sp, -pose.yaw().get().cos() * cp);
        let target = Vec3::new(eye.x + fwd.x, eye.y + fwd.y, eye.z + fwd.z);
        let proj = Mat4::perspective(lens.fov().get(), lens.aspect().get(), lens.near().get(), lens.far().get())
            .unwrap_or(Mat4::IDENTITY);
        let view = Mat4::look_at(eye, target, Vec3::UNIT_Y).unwrap_or(Mat4::IDENTITY);
        proj.multiply(view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Ratio;

    fn origin() -> Pose {
        Pose::new(
            Meters::finite_or_zero(0.0),
            Meters::finite_or_zero(0.0),
            Radians::finite_or_zero(0.0),
            Radians::finite_or_zero(0.0),
        )
    }

    fn intent(
        forward: bool,
        backward: bool,
        strafe_left: bool,
        strafe_right: bool,
        turn_left: bool,
        turn_right: bool,
    ) -> MoveIntent {
        MoveIntent { forward, backward, strafe_left, strafe_right, turn_left, turn_right }
    }

    fn lens() -> Lens {
        Lens::new(
            Radians::finite_or_zero(1.0),
            Ratio::finite_or_zero(1.6),
            Meters::finite_or_zero(0.1),
            Meters::finite_or_zero(500.0),
        )
    }

    #[test]
    fn forward_at_zero_yaw_walks_negative_z() {
        // yaw 0 faces −Z; a forward step decreases z by exactly move_speed.
        let next = FpController::step(
            origin(),
            intent(true, false, false, false, false, false),
            LookDelta::none(),
            WalkTuning::walk(),
        );
        assert!((next.x().get() - 0.0).abs() < 1e-6);
        assert!((next.z().get() - -0.22).abs() < 1e-6);
        assert_eq!(next.yaw().get(), 0.0);
        assert_eq!(next.pitch().get(), 0.0);
    }

    #[test]
    fn backward_reverses_and_strafe_moves_x() {
        let back = FpController::step(
            origin(),
            intent(false, true, false, false, false, false),
            LookDelta::none(),
            WalkTuning::walk(),
        );
        assert!((back.z().get() - 0.22).abs() < 1e-6);

        // Strafe right at yaw 0 moves +X.
        let right = FpController::step(
            origin(),
            intent(false, false, false, true, false, false),
            LookDelta::none(),
            WalkTuning::walk(),
        );
        assert!((right.x().get() - 0.22).abs() < 1e-6);
        assert!(right.z().get().abs() < 1e-6);

        // Strafe left is the mirror.
        let left = FpController::step(
            origin(),
            intent(false, false, true, false, false, false),
            LookDelta::none(),
            WalkTuning::walk(),
        );
        assert!((left.x().get() - -0.22).abs() < 1e-6);
    }

    #[test]
    fn key_turn_and_look_accumulate_yaw() {
        let turned = FpController::step(
            origin(),
            intent(false, false, false, false, true, false),
            LookDelta::none(),
            WalkTuning::walk(),
        );
        assert!((turned.yaw().get() - 0.028).abs() < 1e-6);

        let turned_right = FpController::step(
            origin(),
            intent(false, false, false, false, false, true),
            LookDelta::none(),
            WalkTuning::walk(),
        );
        assert!((turned_right.yaw().get() - -0.028).abs() < 1e-6);

        // Mouse-look yaw adds on top of the key-turn.
        let looked = FpController::step(
            origin(),
            intent(false, false, false, false, true, false),
            LookDelta::new(Radians::finite_or_zero(0.1), Radians::finite_or_zero(0.0)),
            WalkTuning::walk(),
        );
        assert!((looked.yaw().get() - (0.028 + 0.1)).abs() < 1e-6);
    }

    #[test]
    fn pitch_clamps_to_limit() {
        // A huge look-down delta clamps to −pitch_limit; look-up to +pitch_limit.
        let down = FpController::step(
            origin(),
            MoveIntent::default(),
            LookDelta::new(Radians::finite_or_zero(0.0), Radians::finite_or_zero(-10.0)),
            WalkTuning::walk(),
        );
        assert!((down.pitch().get() - -1.45).abs() < 1e-6);

        let up = FpController::step(
            origin(),
            MoveIntent::default(),
            LookDelta::new(Radians::finite_or_zero(0.0), Radians::finite_or_zero(10.0)),
            WalkTuning::walk(),
        );
        assert!((up.pitch().get() - 1.45).abs() < 1e-6);
    }

    #[test]
    fn turned_forward_walks_along_new_yaw() {
        // Turn 90° left (yaw = +π/2) then forward: fx = sin(yaw) = 1, fz = −cos(yaw) = 0,
        // so the step is +X only. Proves movement uses the *new* yaw, this frame.
        let start = Pose::new(
            Meters::finite_or_zero(0.0),
            Meters::finite_or_zero(0.0),
            Radians::finite_or_zero(std::f32::consts::FRAC_PI_2),
            Radians::finite_or_zero(0.0),
        );
        let next = FpController::step(
            start,
            intent(true, false, false, false, false, false),
            LookDelta::none(),
            WalkTuning::walk(),
        );
        assert!((next.x().get() - 0.22).abs() < 1e-6);
        assert!(next.z().get().abs() < 1e-6);
    }

    #[test]
    fn eye_position_seats_above_ground() {
        let eye = FpController::eye_position(
            Pose::new(
                Meters::finite_or_zero(2.0),
                Meters::finite_or_zero(-3.0),
                Radians::finite_or_zero(0.0),
                Radians::finite_or_zero(0.0),
            ),
            Meters::finite_or_zero(5.0),
            WalkTuning::walk(),
        );
        assert_eq!(eye.x, 2.0);
        assert!((eye.y - (5.0 + 1.7)).abs() < 1e-6);
        assert_eq!(eye.z, -3.0);
    }

    #[test]
    fn view_projection_is_finite_and_deterministic() {
        let pose = Pose::new(
            Meters::finite_or_zero(1.0),
            Meters::finite_or_zero(2.0),
            Radians::finite_or_zero(0.3),
            Radians::finite_or_zero(-0.2),
        );
        let vp = FpController::view_projection(pose, Meters::finite_or_zero(0.0), WalkTuning::walk(), lens());
        assert!(vp.as_cols_array().iter().all(|f| f.is_finite()));
        // Same inputs → byte-identical matrix (replayable).
        let vp2 = FpController::view_projection(pose, Meters::finite_or_zero(0.0), WalkTuning::walk(), lens());
        assert_eq!(vp.as_cols_array(), vp2.as_cols_array());
    }

    #[test]
    fn degenerate_lens_falls_back_to_identity_and_stays_finite() {
        // aspect 0 makes `Mat4::perspective` error → identity fallback; the product
        // is still finite. Exercises the `unwrap_or(IDENTITY)` fallback value.
        let bad_lens = Lens::new(
            Radians::finite_or_zero(1.0),
            Ratio::finite_or_zero(0.0),
            Meters::finite_or_zero(0.1),
            Meters::finite_or_zero(500.0),
        );
        let vp = FpController::view_projection(origin(), Meters::finite_or_zero(0.0), WalkTuning::walk(), bad_lens);
        assert!(vp.as_cols_array().iter().all(|f| f.is_finite()));
    }

    #[test]
    fn facade_is_debug() {
        assert!(format!("{:?}", FpController).contains("FpController"));
    }
}
