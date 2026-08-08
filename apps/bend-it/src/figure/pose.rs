//! Poses: the per-tick joint rotations, and the pieces every pose is built from.
//!
//! A [`JointPose`] is one part-local rotation per part plus four **visual root**
//! offsets — a lift, a lateral weight shift, a pitch and a roll. Those four are
//! presentation-only by construction: the rig reads them and nothing else does,
//! so the authoritative ground position and facing that gameplay uses are never
//! nudged by a lean.
//!
//! The gait here is **distance-driven**, not clock-driven: its phase advances
//! with the metres the figure has actually covered. That is the technique that
//! makes a run-up stop skating — if the body slows, the legs slow with it,
//! because they are reading the same number.

use axiom_math::Quat;

use super::model::{
    L_FOOT, L_FOREARM, L_SHIN, L_THIGH, L_UPPER_ARM, PART_COUNT, R_FOOT, R_FOREARM, R_SHIN,
    R_THIGH, R_UPPER_ARM, TORSO,
};

/// A resolved pose.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JointPose {
    pub joints: [Quat; PART_COUNT],
    /// Visual root vertical offset (weight transfer, bob, a dive), metres.
    pub root_lift: f32,
    /// Visual root offset along the facing-right axis, metres.
    pub root_lateral: f32,
    /// Root pitch (forward lean positive), radians.
    pub root_pitch: f32,
    /// Root roll, radians.
    pub root_roll: f32,
}

impl JointPose {
    /// Identity joints, no root adjustment — the base every pose builds on.
    pub fn neutral() -> Self {
        JointPose {
            joints: [Quat::IDENTITY; PART_COUNT],
            root_lift: 0.0,
            root_lateral: 0.0,
            root_pitch: 0.0,
            root_roll: 0.0,
        }
    }

    /// Blend two poses. Used to ease between a run-up and a strike without
    /// either one having to know about the other.
    pub fn blend(a: &JointPose, b: &JointPose, t: f32) -> JointPose {
        let t = t.clamp(0.0, 1.0);
        let mut out = JointPose::neutral();
        (0..PART_COUNT).for_each(|i| {
            out.joints[i] = a.joints[i].nlerp(b.joints[i], t).unwrap_or(b.joints[i]);
        });
        out.root_lift = a.root_lift + (b.root_lift - a.root_lift) * t;
        out.root_lateral = a.root_lateral + (b.root_lateral - a.root_lateral) * t;
        out.root_pitch = a.root_pitch + (b.root_pitch - a.root_pitch) * t;
        out.root_roll = a.root_roll + (b.root_roll - a.root_roll) * t;
        out
    }
}

/// Rotation about X — a limb swinging fore and aft.
pub fn qx(a: f32) -> Quat {
    Quat::from_euler_xyz(a, 0.0, 0.0)
}

/// Rotation about Y — a twist.
pub fn qy(a: f32) -> Quat {
    Quat::from_euler_xyz(0.0, a, 0.0)
}

/// Rotation about Z — a limb spreading sideways.
pub fn qz(a: f32) -> Quat {
    Quat::from_euler_xyz(0.0, 0.0, a)
}

/// Smoothstep.
pub fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A knees-bent athletic crouch, `k` deep.
pub fn crouch(pose: &mut JointPose, k: f32) {
    pose.joints[L_THIGH] = qx(-0.55 * k);
    pose.joints[R_THIGH] = qx(-0.55 * k);
    pose.joints[L_SHIN] = qx(0.90 * k);
    pose.joints[R_SHIN] = qx(0.90 * k);
    pose.joints[L_FOOT] = qx(-0.35 * k);
    pose.joints[R_FOOT] = qx(-0.35 * k);
    pose.root_pitch = 0.22 * k;
    pose.root_lift = -0.14 * k;
}

/// A standing idle with a small ready flex — the shape a player waiting to take
/// a penalty actually holds.
pub fn idle() -> JointPose {
    let mut pose = JointPose::neutral();
    crouch(&mut pose, 0.16);
    pose.joints[L_UPPER_ARM] = qz(-0.14);
    pose.joints[R_UPPER_ARM] = qz(0.14);
    pose.joints[L_FOREARM] = qx(-0.22);
    pose.joints[R_FOREARM] = qx(-0.22);
    pose
}

/// A distance-driven run cycle.
///
/// `distance` is the metres the figure has travelled, `stride` the metres per
/// full cycle, and `intensity` how hard it is running (`0` walk, `1` sprint).
/// Because the phase is metres and not seconds, a figure that decelerates into
/// its plant takes shorter, slower steps on its own.
pub fn run_gait(distance: f32, stride: f32, intensity: f32) -> JointPose {
    let mut pose = JointPose::neutral();
    let intensity = intensity.clamp(0.0, 1.0);
    let phase = (distance / stride.max(0.15)) * core::f32::consts::TAU;
    let (s, c) = (phase.sin(), phase.cos());
    let swing = 0.55 + 0.35 * intensity;

    pose.joints[L_THIGH] = qx(-swing * s);
    pose.joints[R_THIGH] = qx(swing * s);
    // The shin only folds on the recovery half of the stride — a knee that bends
    // while the foot is planted is the classic box-figure skate.
    pose.joints[L_SHIN] = qx((0.85 + 0.45 * intensity) * (s.max(0.0)));
    pose.joints[R_SHIN] = qx((0.85 + 0.45 * intensity) * ((-s).max(0.0)));
    pose.joints[L_FOOT] = qx(-0.28 * s.max(0.0));
    pose.joints[R_FOOT] = qx(-0.28 * (-s).max(0.0));

    // Arms counter-swing against the legs, elbows carried higher the faster it
    // runs.
    pose.joints[L_UPPER_ARM] = Quat::from_euler_xyz(0.75 * s, 0.0, -0.16);
    pose.joints[R_UPPER_ARM] = Quat::from_euler_xyz(-0.75 * s, 0.0, 0.16);
    pose.joints[L_FOREARM] = qx(-0.5 - 0.45 * intensity);
    pose.joints[R_FOREARM] = qx(-0.5 - 0.45 * intensity);

    // The ribcage rotates against the hips, and the body bobs twice a cycle.
    pose.joints[TORSO] = qy(-0.16 * s);
    pose.root_pitch = 0.10 + 0.16 * intensity;
    pose.root_lift = -0.035 - 0.045 * intensity * c.abs();
    pose.root_lateral = 0.022 * c;
    pose
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_neutral_pose_is_identity_everywhere() {
        let pose = JointPose::neutral();
        assert!(pose.joints.iter().all(|q| *q == Quat::IDENTITY));
        assert_eq!(
            (pose.root_lift, pose.root_lateral, pose.root_pitch, pose.root_roll),
            (0.0, 0.0, 0.0, 0.0)
        );
    }

    #[test]
    fn blending_reaches_both_ends_and_moves_in_between() {
        let a = JointPose::neutral();
        let mut b = JointPose::neutral();
        crouch(&mut b, 1.0);
        assert_eq!(JointPose::blend(&a, &b, 0.0).root_pitch, a.root_pitch);
        assert_eq!(JointPose::blend(&a, &b, 1.0).root_pitch, b.root_pitch);
        let mid = JointPose::blend(&a, &b, 0.5);
        assert!(mid.root_pitch > a.root_pitch && mid.root_pitch < b.root_pitch);
        // Out-of-range blends clamp instead of extrapolating.
        assert_eq!(JointPose::blend(&a, &b, 2.0).root_lift, b.root_lift);
        assert_eq!(JointPose::blend(&a, &b, -1.0).root_lift, a.root_lift);
    }

    #[test]
    fn a_crouch_bends_the_knees_and_drops_the_hips() {
        let mut pose = JointPose::neutral();
        crouch(&mut pose, 1.0);
        assert!(pose.root_lift < 0.0);
        assert!(pose.root_pitch > 0.0);
        assert_ne!(pose.joints[L_SHIN], Quat::IDENTITY);
        assert_ne!(idle().joints[L_FOREARM], Quat::IDENTITY);
    }

    #[test]
    fn the_gait_phase_is_metres_not_seconds() {
        let stride = 2.0;
        // Half a stride apart, the legs have swapped.
        let a = run_gait(0.5, stride, 1.0);
        let b = run_gait(1.5, stride, 1.0);
        assert!((a.joints[L_THIGH].x + b.joints[L_THIGH].x).abs() < 1.0e-4);
        // A full stride apart, the pose repeats exactly.
        let c = run_gait(2.5, stride, 1.0);
        assert!((a.joints[L_THIGH].x - c.joints[L_THIGH].x).abs() < 1.0e-4);
        // Standing still is a defined pose, not a divide by zero.
        assert!(run_gait(0.0, 0.0, 0.0).root_lift.is_finite());
        // Running harder swings harder.
        assert!(
            run_gait(0.5, stride, 1.0).joints[L_THIGH].x.abs()
                > run_gait(0.5, stride, 0.0).joints[L_THIGH].x.abs()
        );
    }
}
