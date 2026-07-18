//! A small, explicit two-segment (thigh + shin) analytic leg solver for the
//! locomotion animator — NOT a general IK engine. Given the hip's world
//! placement and a desired world ankle target, it returns the thigh and shin
//! joint quaternions the rig chain consumes, plus the ankle the solve actually
//! reaches (for planted-foot lock-error diagnostics). Everything is derived
//! from geometry (rotations between vectors), so it is free of Euler-sign
//! guesswork, always finite, and clamps unreachable targets instead of
//! stretching or snapping the leg.

use axiom_math::{Quat, Vec3};

use crate::player::model::{L_THIGH, PARTS, R_THIGH};

/// One leg's fixed segment lengths, read from the shared figure model so the
/// solver and the rendered rig can never disagree on limb proportion.
#[derive(Debug, Clone, Copy)]
pub struct LegDims {
    /// Thigh length (hip pivot → knee pivot), yd.
    pub thigh: f32,
    /// Shin length (knee pivot → ankle pivot), yd.
    pub shin: f32,
}

impl LegDims {
    /// The left/right legs share proportions; read them once from the model.
    pub fn from_model() -> Self {
        // Shin's joint offset from the thigh, and the foot's from the shin, are
        // the two segment vectors; their lengths are the bone lengths.
        let thigh = PARTS[L_THIGH + 1].offset.length();
        let shin = PARTS[L_THIGH + 2].offset.length();
        // The right leg is the mirror; assert-free sanity: use the max so a
        // future asymmetric edit can never produce a shorter-than-real reach.
        let thigh = thigh.max(PARTS[R_THIGH + 1].offset.length());
        let shin = shin.max(PARTS[R_THIGH + 2].offset.length());
        LegDims { thigh, shin }
    }

    /// Maximum straight-leg reach, yd.
    pub fn max_reach(self) -> f32 {
        self.thigh + self.shin
    }
}

/// The result of one leg solve.
#[derive(Debug, Clone, Copy)]
pub struct LegSolve {
    /// Thigh joint rotation (local, under the pelvis).
    pub thigh: Quat,
    /// Shin joint rotation (local, under the thigh).
    pub shin: Quat,
    /// The world ankle position the solve actually reaches (== target unless the
    /// target was out of reach and clamped).
    pub ankle: Vec3,
    /// Whether the target was beyond reach and the leg was clamped straight.
    pub clamped: bool,
}

/// A shortest-arc rotation taking unit `from` onto unit `to`. Degenerate cases
/// (parallel / anti-parallel / zero) fall back to identity or a stable
/// perpendicular flip, so the result is always a finite unit quaternion.
fn rotation_between(from: Vec3, to: Vec3) -> Quat {
    let a = from.normalize().unwrap_or(Vec3::new(0.0, -1.0, 0.0));
    let b = to.normalize().unwrap_or(Vec3::new(0.0, -1.0, 0.0));
    let d = a.dot(b).clamp(-1.0, 1.0);
    if d > 0.9999 {
        return Quat::IDENTITY;
    }
    if d < -0.9999 {
        // Anti-parallel: rotate a half-turn about any axis ⟂ a.
        let axis = a.cross(Vec3::UNIT_X);
        let axis = axis
            .normalize()
            .unwrap_or_else(|_| a.cross(Vec3::UNIT_Z).normalize().unwrap_or(Vec3::UNIT_Y));
        return Quat::from_axis_angle(axis, core::f32::consts::PI).unwrap_or(Quat::IDENTITY);
    }
    let axis = a.cross(b);
    Quat::from_axis_angle(axis, d.acos()).unwrap_or(Quat::IDENTITY)
}

/// Solve one leg. `parent_rot` is the world rotation of the pelvis frame the
/// thigh hangs from; `hip_world` is the thigh pivot's world position;
/// `ankle_target` is the desired world ankle position; `knee_forward` is the
/// world direction the knee should bend toward (the player's facing), which
/// disambiguates the two-bone solution and prevents knee inversion.
pub fn solve(
    dims: LegDims,
    parent_rot: Quat,
    hip_world: Vec3,
    ankle_target: Vec3,
    knee_forward: Vec3,
) -> LegSolve {
    let a = dims.thigh.max(1.0e-3);
    let b = dims.shin.max(1.0e-3);
    let reach = a + b;
    let min_reach = (a - b).abs() + 1.0e-3;

    // Work in the pelvis-local frame: rotations composed by the rig are all
    // expressed there, so the returned joint quats drop straight into the chain.
    let inv = parent_rot.inverse().unwrap_or(Quat::IDENTITY);
    let to_target = inv.rotate(ankle_target.subtract(hip_world));
    let forward_local = inv.rotate(knee_forward);

    let raw_len = to_target.length();
    let clamped = raw_len > reach - 1.0e-3 || raw_len < min_reach;
    let d = raw_len.clamp(min_reach, reach - 1.0e-3);
    let dir = to_target.normalize().unwrap_or(Vec3::new(0.0, -1.0, 0.0));

    // Knee hinge axis: perpendicular to both the hip→target line and the
    // forward bend direction. A stable fallback keeps it finite when the target
    // is (near) parallel to forward.
    let hinge = dir.cross(forward_local);
    let hinge = hinge
        .normalize()
        .unwrap_or_else(|_| dir.cross(Vec3::UNIT_X).normalize().unwrap_or(Vec3::UNIT_X));

    // Angle between the hip→target line and the thigh (law of cosines).
    let cos_hip = ((a * a + d * d - b * b) / (2.0 * a * d)).clamp(-1.0, 1.0);
    let hip_angle = cos_hip.acos();

    // Thigh points along `dir` rotated toward `forward` (the knee leads), by
    // `hip_angle` about the hinge — a rotation about `dir × forward` swings the
    // thigh from the hip→ankle line toward the forward bend, so the knee never
    // inverts.
    let lift = Quat::from_axis_angle(hinge, hip_angle).unwrap_or(Quat::IDENTITY);
    let thigh_dir = lift.rotate(dir).normalize().unwrap_or(dir);
    let knee_world = hip_world.add(parent_rot.rotate(thigh_dir.mul_scalar(a)));

    // Shin runs from the knee to the (clamped) ankle target.
    let ankle = hip_world.add(parent_rot.rotate(dir.mul_scalar(d)));
    let shin_dir = ankle
        .subtract(knee_world)
        .normalize()
        .map(|w| inv.rotate(w))
        .unwrap_or(thigh_dir);

    // Rest bone direction is straight down (−Y) in each joint's local frame.
    let rest = Vec3::new(0.0, -1.0, 0.0);
    let thigh = rotation_between(rest, thigh_dir);
    // Shin rotation is relative to the thigh, so express the world shin
    // direction back in the thigh's local frame.
    let shin_in_thigh = thigh.inverse().unwrap_or(Quat::IDENTITY).rotate(shin_dir);
    let shin = rotation_between(rest, shin_in_thigh);

    LegSolve {
        thigh,
        shin,
        ankle,
        clamped,
    }
}

/// Forward-kinematics ankle position for a solved leg — the independent check
/// the planted-foot lock-error diagnostic (and its tests) use.
pub fn ankle_world(
    dims: LegDims,
    parent_rot: Quat,
    hip_world: Vec3,
    thigh: Quat,
    shin: Quat,
) -> Vec3 {
    let rest = Vec3::new(0.0, -1.0, 0.0);
    let thigh_dir = parent_rot.rotate(thigh.rotate(rest.mul_scalar(dims.thigh)));
    let knee = hip_world.add(thigh_dir);
    let shin_dir = parent_rot.rotate(thigh.multiply(shin).rotate(rest.mul_scalar(dims.shin)));
    knee.add(shin_dir)
}
