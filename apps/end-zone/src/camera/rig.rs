//! The camera rig: critically damped spring smoothing of the directed base
//! pose, advanced on fixed simulation ticks. The rig holds ONLY the base —
//! impulses are added after smoothing and can never drift it.

use axiom::prelude::Vec3;

use crate::config::DT;
use crate::data::CameraTuning;

use super::modes::CameraPose;

/// One critically damped 3D spring channel.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Spring3 {
    pos: Vec3,
    vel: Vec3,
}

impl Spring3 {
    fn snap_to(target: Vec3) -> Self {
        Spring3 {
            pos: target,
            vel: Vec3::ZERO,
        }
    }

    /// Semi-implicit critically damped step toward `target`.
    fn step(&mut self, target: Vec3, omega: f32, zeta: f32) {
        let accel = target
            .subtract(self.pos)
            .mul_scalar(omega * omega)
            .subtract(self.vel.mul_scalar(2.0 * zeta * omega));
        self.vel = self.vel.add(accel.mul_scalar(DT));
        self.pos = self.pos.add(self.vel.mul_scalar(DT));
    }
}

/// The smoothed base camera.
#[derive(Debug, Clone, PartialEq)]
pub struct CameraRig {
    eye: Spring3,
    target: Spring3,
    fov: f32,
    fov_vel: f32,
    initialized: bool,
}

impl CameraRig {
    pub fn new() -> Self {
        CameraRig {
            eye: Spring3::snap_to(Vec3::new(0.0, 20.0, -40.0)),
            target: Spring3::snap_to(Vec3::ZERO),
            fov: 60.0,
            fov_vel: 0.0,
            initialized: false,
        }
    }

    /// Snap the rig to a pose (play reset / first frame — no smoothing).
    pub fn snap(&mut self, pose: CameraPose) {
        self.eye = Spring3::snap_to(pose.eye);
        self.target = Spring3::snap_to(pose.target);
        self.fov = pose.fov_degrees;
        self.fov_vel = 0.0;
        self.initialized = true;
    }

    /// Advance one fixed tick toward `desired`, returning the smoothed base.
    pub fn step(&mut self, desired: CameraPose, tuning: &CameraTuning) -> CameraPose {
        if !self.initialized {
            self.snap(desired);
        }
        let omega = core::f32::consts::TAU * tuning.spring_frequency;
        let zeta = tuning.damping_ratio;
        self.eye.step(desired.eye, omega, zeta);
        self.target.step(desired.target, omega, zeta);
        let fov_accel =
            (desired.fov_degrees - self.fov) * omega * omega - 2.0 * zeta * omega * self.fov_vel;
        self.fov_vel += fov_accel * DT;
        self.fov += self.fov_vel * DT;
        CameraPose {
            eye: self.eye.pos,
            target: self.target.pos,
            fov_degrees: self.fov,
        }
    }

    /// The current smoothed base pose (read-only; used by tests to prove
    /// impulses never touch the base).
    pub fn base(&self) -> CameraPose {
        CameraPose {
            eye: self.eye.pos,
            target: self.target.pos,
            fov_degrees: self.fov,
        }
    }
}

impl Default for CameraRig {
    fn default() -> Self {
        CameraRig::new()
    }
}
