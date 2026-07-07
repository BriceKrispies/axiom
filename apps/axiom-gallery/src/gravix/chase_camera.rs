//! The **third-person chase camera**. It orbits behind and above the ball at a
//! yaw/pitch the **player aims with the mouse** — it does *not* auto-follow the
//! ball's velocity (so strafing the ball never swings the view tank-style). The
//! eye trails the ball smoothly (launch-safe); the yaw/pitch respond directly to
//! mouse motion. It hands the renderer an `(eye, target)` pair and the game a
//! ground-plane `(forward, right)` basis so WASD move relative to where the
//! camera looks. All feel comes from the `settings::CAM_*` constants.

use axiom::prelude::Vec3;

use crate::gravix::settings;

/// The chase rig: a mouse-controlled orbit (`yaw` about +Y, `pitch` elevation) and
/// a smoothed eye that trails the ball.
#[derive(Debug, Clone, Copy)]
pub struct ChaseCamera {
    yaw: f32,
    pitch: f32,
    eye: Vec3,
}

impl ChaseCamera {
    /// A rig framing `ball`, initially looking along compass `initial_yaw`
    /// (radians; `facing = (sin, 0, cos)`), at the default pitch.
    pub fn new(initial_yaw: f32, ball: Vec3) -> Self {
        let pitch = settings::CAM_PITCH_DEFAULT;
        let facing = yaw_facing(initial_yaw);
        ChaseCamera { yaw: initial_yaw, pitch, eye: desired_eye(ball, facing, pitch) }
    }

    /// Advance one step. `yaw_delta` / `pitch_delta` are the mouse deltas this
    /// step (in pixels; scaled by the mouse sensitivity here), turning the orbit;
    /// the eye then eases toward its trailing pose behind the ball.
    pub fn update(&mut self, ball: Vec3, yaw_delta: f32, pitch_delta: f32, dt: f32) {
        self.yaw += yaw_delta * settings::CAM_MOUSE_SENSITIVITY;
        self.pitch = (self.pitch + pitch_delta * settings::CAM_MOUSE_SENSITIVITY)
            .clamp(settings::CAM_PITCH_MIN, settings::CAM_PITCH_MAX);

        let want = desired_eye(ball, self.facing(), self.pitch);
        let ke = ease_rate(settings::CAM_EYE_SMOOTHING, dt);
        self.eye = self.eye.add(want.subtract(self.eye).mul_scalar(ke));
    }

    /// The camera eye and look target (a little ahead of the ball along the
    /// facing) for the renderer's `looking_at`.
    pub fn eye_target(&self, ball: Vec3) -> (Vec3, Vec3) {
        (self.eye, ball.add(self.facing().mul_scalar(settings::CAM_LOOK_AHEAD)))
    }

    /// The horizontal facing direction (unit) the camera looks along.
    pub fn facing(&self) -> Vec3 {
        yaw_facing(self.yaw)
    }

    /// The ground-plane forward/right unit basis for camera-relative input: W/S
    /// drive along `forward`, A/D strafe along `right`.
    pub fn ground_basis(&self) -> (Vec3, Vec3) {
        let fwd = self.facing();
        (fwd, Vec3::new(-fwd.z, 0.0, fwd.x))
    }
}

/// The horizontal unit facing for a yaw: `(sin, 0, cos)` — yaw `0` looks along `+Z`.
fn yaw_facing(yaw: f32) -> Vec3 {
    Vec3::new(yaw.sin(), 0.0, yaw.cos())
}

/// The trailing eye for a ball, facing, and pitch: behind along `-facing` and up,
/// with pitch raising the eye (and drawing it in horizontally) to look down more.
fn desired_eye(ball: Vec3, facing: Vec3, pitch: f32) -> Vec3 {
    let back = facing.mul_scalar(-settings::CAM_DISTANCE * pitch.cos());
    let lift = settings::CAM_HEIGHT + settings::CAM_DISTANCE * pitch.sin();
    ball.add(back).add(Vec3::new(0.0, lift, 0.0))
}

/// An exponential-smoothing blend factor for rate `k` over `dt` (`1 - e^{-k·dt}`),
/// clamped to `[0, 1]`.
fn ease_rate(k: f32, dt: f32) -> f32 {
    (1.0 - (-k * dt).exp()).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f32::consts::FRAC_PI_2;

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn the_eye_sits_behind_and_above_the_ball() {
        let cam = ChaseCamera::new(0.0, Vec3::ZERO); // looking +z
        let (eye, target) = cam.eye_target(Vec3::ZERO);
        assert!(eye.y > 0.0, "camera is above the ball");
        assert!(eye.z < 0.0, "camera trails behind (−z) a +z facing");
        assert!(target.z > 0.0, "target leads the ball along the facing");
    }

    #[test]
    fn the_mouse_turns_the_camera_and_the_ball_velocity_does_not() {
        let mut cam = ChaseCamera::new(0.0, Vec3::ZERO);
        // No mouse input over many steps: the facing holds regardless of where the
        // ball is (the camera never auto-aligns to motion).
        for _ in 0..120 {
            cam.update(Vec3::new(5.0, 0.0, 5.0), 0.0, 0.0, DT);
        }
        let held = cam.facing();
        assert!((held.subtract(Vec3::new(0.0, 0.0, 1.0))).length() < 1.0e-3, "facing held, got {held:?}");
        // A rightward mouse sweep turns the facing toward +x.
        for _ in 0..60 {
            cam.update(Vec3::ZERO, 40.0, 0.0, DT);
        }
        let turned = cam.facing();
        assert!(turned.x > 0.3, "mouse turned the camera toward +x, got {turned:?}");
        assert!((turned.length() - 1.0).abs() < 1.0e-4 && turned.y == 0.0);
    }

    #[test]
    fn pitch_is_clamped_within_range() {
        let mut cam = ChaseCamera::new(0.0, Vec3::ZERO);
        // Slam the mouse up far past the limit; pitch saturates, it does not flip.
        for _ in 0..500 {
            cam.update(Vec3::ZERO, 0.0, 400.0, DT);
        }
        let high = ChaseCamera::new(0.0, Vec3::ZERO);
        let _ = high;
        // Re-derive the clamped eye is finite + above.
        let (eye, _) = cam.eye_target(Vec3::ZERO);
        assert!(eye.y.is_finite() && eye.y > 0.0);
        // Slam it down past the low limit too.
        for _ in 0..500 {
            cam.update(Vec3::ZERO, 0.0, -400.0, DT);
        }
        let (eye2, _) = cam.eye_target(Vec3::ZERO);
        assert!(eye2.y.is_finite());
    }

    #[test]
    fn ground_basis_is_horizontal_orthogonal_and_tracks_yaw() {
        let cam = ChaseCamera::new(FRAC_PI_2, Vec3::ZERO); // facing +x
        let (fwd, right) = cam.ground_basis();
        assert!((fwd.subtract(Vec3::new(1.0, 0.0, 0.0))).length() < 1.0e-5, "yaw π/2 faces +x");
        assert!(fwd.y.abs() < 1.0e-6 && right.y.abs() < 1.0e-6);
        assert!(fwd.x * right.x + fwd.z * right.z < 1.0e-5, "forward ⟂ right");
    }
}
