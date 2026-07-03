//! Anatomical joint limits and the [`PoseSolver`] that enforces them.
//!
//! A [`JointLimit`] is a per-axis min/max on a bone's Euler rotation (radians).
//! The solver clamps every joint of a [`Pose`] into its limit, so an authored or
//! interpolated rotation that would bend a knee or elbow backward is pulled back
//! to the joint's legal range. Clamping is pure per-axis arithmetic — branchless
//! and order-independent — and [`PoseSolver::is_legal`] reports whether a pose is
//! already within limits. Expects one limit per bone, index-aligned with the
//! pose.

use axiom_math::Vec3;

use crate::pose::Pose;

/// A per-axis rotation range (radians) for one bone. A hinge like a knee gets a
/// near-zero range on two axes and a one-sided range on its bend axis (e.g.
/// `min.x = 0`) so it cannot hyperextend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JointLimit {
    /// Minimum Euler rotation per axis (radians).
    pub min: Vec3,
    /// Maximum Euler rotation per axis (radians).
    pub max: Vec3,
}

impl JointLimit {
    /// Construct a limit from a per-axis min and max. `min <= max` per axis is
    /// expected.
    pub const fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    /// An effectively free joint: ±τ (one full turn) on every axis. Used for
    /// bones with no anatomical constraint (spine twist, the root, etc.).
    pub fn free() -> Self {
        let b = std::f32::consts::TAU;
        Self {
            min: Vec3::new(-b, -b, -b),
            max: Vec3::new(b, b, b),
        }
    }

    /// Clamp `euler` into this limit, per axis.
    pub fn clamp(&self, euler: Vec3) -> Vec3 {
        Vec3::new(
            euler.x.clamp(self.min.x, self.max.x),
            euler.y.clamp(self.min.y, self.max.y),
            euler.z.clamp(self.min.z, self.max.z),
        )
    }

    /// Whether `euler` is already within this limit on every axis.
    pub fn contains(&self, euler: Vec3) -> bool {
        (euler.x >= self.min.x)
            & (euler.x <= self.max.x)
            & (euler.y >= self.min.y)
            & (euler.y <= self.max.y)
            & (euler.z >= self.min.z)
            & (euler.z <= self.max.z)
    }
}

/// Enforces a per-bone [`JointLimit`] table over a [`Pose`]. One limit per bone,
/// index-aligned with the pose's joints.
#[derive(Debug, Clone, PartialEq)]
pub struct PoseSolver {
    limits: Vec<JointLimit>,
}

impl PoseSolver {
    /// A solver over a per-bone limit table.
    pub fn new(limits: Vec<JointLimit>) -> Self {
        Self { limits }
    }

    /// The number of bones this solver constrains.
    pub fn bone_count(&self) -> usize {
        self.limits.len()
    }

    /// Return a copy of `pose` with every joint clamped into its limit. Bends
    /// that would drive a hinge backward (a negative knee/elbow angle) are pulled
    /// to the joint's minimum.
    pub fn solve(&self, pose: &Pose) -> Pose {
        Pose::new(
            pose.joint_eulers
                .iter()
                .enumerate()
                .map(|(i, &euler)| self.limits[i].clamp(euler))
                .collect(),
        )
    }

    /// Whether every joint of `pose` is already within its limit.
    pub fn is_legal(&self, pose: &Pose) -> bool {
        pose.joint_eulers
            .iter()
            .enumerate()
            .all(|(i, &euler)| self.limits[i].contains(euler))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{ApproxEq, Epsilon};

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-6).unwrap()
    }

    /// A one-sided hinge about X: cannot go negative (no backward bend).
    fn knee() -> JointLimit {
        JointLimit::new(
            Vec3::new(0.0, -0.02, -0.02),
            Vec3::new(2.4, 0.02, 0.02),
        )
    }

    #[test]
    fn free_limit_contains_everything_reasonable() {
        let f = JointLimit::free();
        assert!(f.contains(Vec3::new(3.0, -3.0, 1.5)));
        assert!(f.clamp(Vec3::new(1.0, 2.0, 3.0)).approx_eq(&Vec3::new(1.0, 2.0, 3.0), eps()));
    }

    #[test]
    fn hinge_clamps_backward_bend_to_min() {
        let k = knee();
        // A backward (negative-X) bend is illegal and clamps up to 0.
        assert!(!k.contains(Vec3::new(-0.5, 0.0, 0.0)));
        let clamped = k.clamp(Vec3::new(-0.5, 0.0, 0.0));
        assert!(clamped.x >= 0.0);
        assert!(clamped.approx_eq(&Vec3::new(0.0, 0.0, 0.0), eps()));
        // A forward bend within range is preserved.
        assert!(k.contains(Vec3::new(1.0, 0.0, 0.0)));
    }

    #[test]
    fn contains_rejects_each_axis_out_of_range() {
        let k = knee();
        assert!(!k.contains(Vec3::new(3.0, 0.0, 0.0))); // x too high
        assert!(!k.contains(Vec3::new(1.0, 0.5, 0.0))); // y too high
        assert!(!k.contains(Vec3::new(1.0, -0.5, 0.0))); // y too low
        assert!(!k.contains(Vec3::new(1.0, 0.0, 0.5))); // z too high
        assert!(!k.contains(Vec3::new(1.0, 0.0, -0.5))); // z too low
    }

    #[test]
    fn solver_clamps_pose_and_reports_legality() {
        let solver = PoseSolver::new(vec![JointLimit::free(), knee()]);
        assert_eq!(solver.bone_count(), 2);
        // Bone 1 bent backward: illegal, then solved legal.
        let bad = Pose::new(vec![Vec3::ZERO, Vec3::new(-1.0, 0.0, 0.0)]);
        assert!(!solver.is_legal(&bad));
        let fixed = solver.solve(&bad);
        assert!(solver.is_legal(&fixed));
        assert!(fixed.joint_eulers[1].x >= 0.0);
    }
}
