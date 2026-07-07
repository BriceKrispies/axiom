//! The **third-person chase camera**. It trails behind and slightly above the
//! ball, turns its facing toward the ball's horizontal velocity when moving fast
//! (but holds a stable facing at low speed so it never spins randomly), looks a
//! little ahead of the ball, and smooths both its facing and its eye so a spin
//! launch never snaps the view. Pure math — it hands the renderer an
//! `(eye, target)` pair and the game a ground-plane `(forward, right)` basis for
//! camera-relative input. All feel comes from the `settings::CAM_*` constants.

use axiom::prelude::Vec3;

use crate::gravix::settings;

/// The chase rig: a smoothed horizontal facing direction and a smoothed eye.
#[derive(Debug, Clone, Copy)]
pub struct ChaseCamera {
    facing: Vec3,
    eye: Vec3,
}

impl ChaseCamera {
    /// A rig framing `ball`, initially facing `initial_facing` (any vector; the
    /// horizontal component is used, defaulting to `+Z`).
    pub fn new(initial_facing: Vec3, ball: Vec3) -> Self {
        let facing = horizontal_unit(initial_facing);
        ChaseCamera { facing, eye: desired_eye(ball, facing) }
    }

    /// Advance one step: turn the facing toward the ball's horizontal velocity when
    /// fast enough, hold it when slow, and ease the eye toward its trailing pose.
    pub fn update(&mut self, ball: Vec3, ball_velocity: Vec3, dt: f32) {
        let speed = horizontal_speed(ball_velocity);
        let desired_facing = if speed > settings::CAM_ALIGN_MIN_SPEED {
            horizontal_unit(ball_velocity)
        } else {
            self.facing
        };
        let kf = ease_rate(settings::CAM_FACING_SMOOTHING, dt);
        self.facing = horizontal_unit(self.facing.add(desired_facing.subtract(self.facing).mul_scalar(kf)));

        let want = desired_eye(ball, self.facing);
        let ke = ease_rate(settings::CAM_EYE_SMOOTHING, dt);
        self.eye = self.eye.add(want.subtract(self.eye).mul_scalar(ke));
    }

    /// The camera eye and the look target (a little ahead of the ball along the
    /// facing, not the ball centre) for the renderer's `looking_at`.
    pub fn eye_target(&self, ball: Vec3) -> (Vec3, Vec3) {
        (self.eye, ball.add(self.facing.mul_scalar(settings::CAM_LOOK_AHEAD)))
    }

    /// The horizontal facing direction (unit).
    pub fn facing(&self) -> Vec3 {
        self.facing
    }

    /// The ground-plane forward/right unit basis for camera-relative input.
    pub fn ground_basis(&self) -> (Vec3, Vec3) {
        let fwd = self.facing;
        (fwd, Vec3::new(-fwd.z, 0.0, fwd.x))
    }
}

/// The trailing eye for a ball and facing: behind along `-facing`, up by the height.
fn desired_eye(ball: Vec3, facing: Vec3) -> Vec3 {
    ball.subtract(facing.mul_scalar(settings::CAM_DISTANCE))
        .add(Vec3::new(0.0, settings::CAM_HEIGHT, 0.0))
}

/// A horizontal unit vector from `v` (Y flattened), defaulting to `+Z` when the
/// horizontal projection is degenerate.
fn horizontal_unit(v: Vec3) -> Vec3 {
    let flat = Vec3::new(v.x, 0.0, v.z);
    let len = (flat.x * flat.x + flat.z * flat.z).sqrt();
    if len < 1.0e-5 {
        Vec3::new(0.0, 0.0, 1.0)
    } else {
        flat.mul_scalar(1.0 / len)
    }
}

fn horizontal_speed(v: Vec3) -> f32 {
    (v.x * v.x + v.z * v.z).sqrt()
}

/// An exponential-smoothing blend factor for rate `k` over `dt` (`1 - e^{-k·dt}`),
/// clamped to `[0, 1]`.
fn ease_rate(k: f32, dt: f32) -> f32 {
    (1.0 - (-k * dt).exp()).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn the_eye_sits_behind_and_above_the_ball() {
        let cam = ChaseCamera::new(Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO);
        let (eye, target) = cam.eye_target(Vec3::ZERO);
        assert!(eye.y > 0.0, "camera is above the ball");
        assert!(eye.z < 0.0, "camera trails behind (−z) a +z facing");
        // Looks ahead of the ball, not at its centre.
        assert!(target.z > 0.0, "target leads the ball along the facing");
    }

    #[test]
    fn facing_is_stable_at_low_speed_and_aligns_to_velocity_when_fast() {
        let mut cam = ChaseCamera::new(Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO);
        // Near-zero velocity: facing holds (no random spin) over many steps.
        for _ in 0..120 {
            cam.update(Vec3::ZERO, Vec3::new(0.01, 0.0, 0.0), DT);
        }
        let f = cam.facing();
        assert!((f.subtract(Vec3::new(0.0, 0.0, 1.0))).length() < 1.0e-3, "held facing, got {f:?}");
        // Fast velocity toward +x: facing turns toward +x and stays unit + horizontal.
        for _ in 0..240 {
            cam.update(Vec3::ZERO, Vec3::new(20.0, -3.0, 0.0), DT);
        }
        let g = cam.facing();
        assert!(g.x > 0.9 && g.y.abs() < 1.0e-6, "aligned to +x velocity, got {g:?}");
        assert!((g.length() - 1.0).abs() < 1.0e-4);
    }

    #[test]
    fn ground_basis_is_horizontal_and_orthogonal() {
        let cam = ChaseCamera::new(Vec3::new(1.0, 2.0, 1.0), Vec3::ZERO);
        let (fwd, right) = cam.ground_basis();
        assert!(fwd.y.abs() < 1.0e-6 && right.y.abs() < 1.0e-6);
        assert!(fwd.x * right.x + fwd.z * right.z < 1.0e-5, "forward ⟂ right");
    }

    #[test]
    fn degenerate_velocity_keeps_a_defined_facing() {
        // A camera created facing straight up (no horizontal component) defaults to +z.
        let cam = ChaseCamera::new(Vec3::UNIT_Y, Vec3::ZERO);
        assert_eq!(cam.facing(), Vec3::new(0.0, 0.0, 1.0));
    }
}
