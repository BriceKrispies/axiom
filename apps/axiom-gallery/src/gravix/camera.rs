//! The third-person orbit camera. A yaw/pitch rig that circles the marble at a
//! fixed distance; the marble's own spin never affects the rig (the camera is
//! steered only by the arrow-key orbit input). Pure math — `(eye, target)` pairs
//! the renderer turns into a `looking_at` transform.

use axiom::prelude::Vec3;

use crate::gravix::settings;

/// The orbit rig state: horizontal `yaw` and vertical `pitch` (radians).
#[derive(Clone, Copy, Debug)]
pub struct OrbitCamera {
    pub yaw: f32,
    pub pitch: f32,
}

impl OrbitCamera {
    /// A rig aimed with the given yaw and the default initial pitch.
    pub fn new(yaw: f32) -> Self {
        OrbitCamera {
            yaw,
            pitch: settings::CAMERA_INITIAL_PITCH,
        }
    }

    /// Advance the orbit by the given yaw / pitch deltas (radians), clamping pitch
    /// to its limits.
    pub fn steer(&mut self, yaw_delta: f32, pitch_delta: f32) {
        self.yaw += yaw_delta;
        self.pitch = (self.pitch + pitch_delta).clamp(
            settings::CAMERA_PITCH_MIN,
            settings::CAMERA_PITCH_MAX,
        );
    }

    /// The camera eye and look-target for a marble at `marble`. At `yaw == 0` the
    /// eye sits behind the marble along `-Z` (courses run toward `+Z`).
    pub fn eye_target(&self, marble: Vec3) -> (Vec3, Vec3) {
        let d = settings::CAMERA_DISTANCE;
        let cp = self.pitch.cos();
        let sp = self.pitch.sin();
        let sy = self.yaw.sin();
        let cy = self.yaw.cos();
        let offset = Vec3::new(-d * cp * sy, d * sp, -d * cp * cy);
        (marble.add(offset), marble)
    }

    /// The camera's ground-plane forward and right unit vectors (Y flattened),
    /// used to make marble steering camera-relative.
    pub fn ground_basis(&self, marble: Vec3) -> (Vec3, Vec3) {
        let (eye, target) = self.eye_target(marble);
        let mut fwd = Vec3::new(target.x - eye.x, 0.0, target.z - eye.z);
        let len = (fwd.x * fwd.x + fwd.z * fwd.z).sqrt();
        fwd = choose_forward(fwd, len);
        // right = forward × worldUp (on the ground plane): (fx,0,fz) × (0,1,0).
        let right = Vec3::new(-fwd.z, 0.0, fwd.x);
        (fwd, right)
    }
}

/// A normalized ground forward, defaulting to `-Z` when the camera sits directly
/// over the marble (degenerate horizontal projection).
fn choose_forward(fwd: Vec3, len: f32) -> Vec3 {
    if len < 1.0e-5 {
        Vec3::new(0.0, 0.0, -1.0)
    } else {
        Vec3::new(fwd.x / len, 0.0, fwd.z / len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eye_sits_behind_and_above_the_marble_at_zero_yaw() {
        let cam = OrbitCamera::new(0.0);
        let (eye, target) = cam.eye_target(Vec3::ZERO);
        assert!(eye.y > target.y, "camera is above the marble");
        assert!(eye.z < 0.0, "camera sits behind (-Z) at yaw 0");
        assert_eq!(target, Vec3::ZERO);
    }

    #[test]
    fn pitch_is_clamped() {
        let mut cam = OrbitCamera::new(0.0);
        cam.steer(0.0, 100.0);
        assert!(cam.pitch <= settings::CAMERA_PITCH_MAX + 1.0e-6);
        cam.steer(0.0, -100.0);
        assert!(cam.pitch >= settings::CAMERA_PITCH_MIN - 1.0e-6);
    }

    #[test]
    fn ground_basis_is_horizontal_and_orthogonal() {
        let cam = OrbitCamera::new(0.7);
        let (fwd, right) = cam.ground_basis(Vec3::new(1.0, 2.0, 3.0));
        assert!(fwd.y.abs() < 1.0e-6 && right.y.abs() < 1.0e-6);
        assert!(fwd.x * right.x + fwd.z * right.z < 1.0e-5, "forward ⟂ right");
    }
}
