//! Planted-foot targeting: each foot's stance phases and this tick's world
//! ankle target. On foot-strike the world ground contact is latched and held
//! fixed while the body travels over it (the anti-skate); through swing the foot
//! arcs to its next committed landing. Pure geometry — no simulation access.

use axiom::prelude::Vec3;

use crate::data::LocomotionTuning;
use crate::player::model::hip_half_width;

/// Ankle rest height above the field when a foot is planted (from the model's
/// foot box: sole on the ground puts the ankle pivot this far up), yd.
pub const ANKLE_GROUND_OFFSET: f32 = 0.09;

/// A single foot's stance phase within its step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FootPhase {
    Planted,
    PushOff,
    Swing,
    Landing,
}

/// One foot's persistent placement state.
#[derive(Debug, Clone, Copy)]
pub struct Foot {
    pub phase: FootPhase,
    /// The world-space ground point the ankle tracks while planted.
    pub lock: Vec3,
    /// The next landing point, committed at push-off and held through swing.
    pub pending: Vec3,
    /// The lift-off world position a swing interpolates from.
    pub swing_from: Vec3,
    /// This tick's resolved ankle world target (planted lock, or swing arc).
    pub target: Vec3,
    /// The world ankle the leg solve actually reached (filled by the pose pass).
    pub ankle: Vec3,
    /// Distance between `target` and the solved `ankle` (planted-foot slide).
    pub lock_error: f32,
}

impl Foot {
    pub fn at(ground: Vec3) -> Self {
        let g = Vec3::new(ground.x, ground.y + ANKLE_GROUND_OFFSET, ground.z);
        Foot {
            phase: FootPhase::Planted,
            lock: ground,
            pending: ground,
            swing_from: ground,
            target: g,
            ankle: g,
            lock_error: 0.0,
        }
    }
}

/// The (right, forward) unit directions for a facing yaw.
pub fn dirs(facing: f32) -> (Vec3, Vec3) {
    let forward = Vec3::new(facing.sin(), 0.0, facing.cos());
    let right = Vec3::new(facing.cos(), 0.0, -facing.sin());
    (right, forward)
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

/// The planted fraction at a given stride: capped so the body only travels
/// `2·stance_reach` while a foot is down, so the world-locked foot never
/// over-extends the leg (it lifts into swing first). Short strides keep the full
/// configured stance; long (sprint) strides shrink it toward brief ground
/// contact.
pub fn planted_fraction(stride: f32, tuning: &LocomotionTuning) -> f32 {
    let reach_bound = (2.0 * tuning.stance_reach) / stride.max(0.1);
    reach_bound.min(tuning.planted_fraction).max(0.12)
}

/// Resolve both feet: local phases (offset by ½ a cycle), world ankle targets,
/// and the primary planted foot. Returns `true` when the LEFT foot is the
/// primary planted one. (Cohesive per-tick foot-placement inputs — not a bag.)
#[allow(clippy::too_many_arguments)]
pub fn resolve(
    phase: f32,
    left: &mut Foot,
    right: &mut Foot,
    ground: Vec3,
    facing: f32,
    stride: f32,
    turn_intensity: f32,
    norm_speed: f32,
    tuning: &LocomotionTuning,
) -> bool {
    let (right_dir, forward) = dirs(facing);
    // The PLANT is laterally free: it converges inside the hips at a normal
    // stride and widens through a turn. The SWING is not — see `step_foot`.
    let widen = tuning.stance_half_width + tuning.turn_widen * turn_intensity;
    let hip = hip_half_width();
    let pf = planted_fraction(stride, tuning);

    let left_lp = phase;
    let right_lp = (phase + 0.5).rem_euclid(1.0);
    let lat_l = right_dir.mul_scalar(-widen);
    let lat_r = right_dir.mul_scalar(widen);
    let hip_l = right_dir.mul_scalar(-hip);
    let hip_r = right_dir.mul_scalar(hip);

    let l = FootStep {
        lp: left_lp,
        lat: lat_l,
        hip_lat: hip_l,
    };
    let r = FootStep {
        lp: right_lp,
        lat: lat_r,
        hip_lat: hip_r,
    };
    step_foot(left, l, ground, forward, pf, norm_speed, tuning);
    step_foot(right, r, ground, forward, pf, norm_speed, tuning);

    // Primary planted foot = whichever is in stance (earlier local phase wins a
    // brief double-support overlap).
    (left_lp < pf) && (right_lp >= pf || left_lp <= right_lp)
}

/// One foot's per-tick placement geometry: its local phase and the two lateral
/// anchors the step is built from — the free `lat` the foot PLANTS at, and the
/// `hip_lat` its own hip actually sits above, which the tucked mid-swing foot is
/// held on (see `step_foot`).
#[derive(Debug, Clone, Copy)]
struct FootStep {
    lp: f32,
    lat: Vec3,
    hip_lat: Vec3,
}

/// Advance one foot: latch its world contact at strike, hold it while the body
/// travels over it, and arc it forward through swing to the next reach-bounded
/// landing. `forward`/`lat` place the landing a small, always-solvable distance
/// from the hip so the planted foot never has to slide.
fn step_foot(
    foot: &mut Foot,
    step: FootStep,
    ground: Vec3,
    forward: Vec3,
    pf: f32,
    norm_speed: f32,
    tuning: &LocomotionTuning,
) {
    let FootStep { lp, lat, hip_lat } = step;
    let ground_y = ground.y + ANKLE_GROUND_OFFSET;
    // The reach-bounded landing point: a short step ahead of the hip.
    let landing = ground.add(forward.mul_scalar(tuning.stance_reach)).add(lat);
    if lp < pf {
        // Stance: a fresh strike latches the world lock; it then stays fixed.
        let just_landed = matches!(foot.phase, FootPhase::Swing | FootPhase::PushOff);
        if just_landed {
            foot.lock = foot.pending;
        }
        let zone = pf * 0.2;
        foot.phase = if lp < zone {
            FootPhase::Landing
        } else if lp > pf - zone {
            FootPhase::PushOff
        } else {
            FootPhase::Planted
        };
        foot.target = Vec3::new(foot.lock.x, ground_y, foot.lock.z);
    } else {
        // Swing: track the moving hip's landing point (so the plant is always
        // reachable), arcing up from the lift-off position.
        let just_lifted = !matches!(foot.phase, FootPhase::Swing);
        if just_lifted {
            foot.swing_from = foot.lock;
        }
        foot.pending = landing;
        foot.phase = FootPhase::Swing;
        let s = ((lp - pf) / (1.0 - pf)).clamp(0.0, 1.0);
        let arc = (s * core::f32::consts::PI).sin();
        // Flat glide from lift-off to the next landing — a walk's low swing.
        let glide_x = lerp(foot.swing_from.x, foot.pending.x, s);
        let glide_z = lerp(foot.swing_from.z, foot.pending.z, s);
        // Knee drive: at speed, aim the mid-swing foot at a point FORWARD of and
        // HIGH above the hip, so the thigh lifts the knee up in front (a high
        // knee) rather than the foot skimming forward low or tucking straight
        // under. Bell-shaped over the swing (0 at strike/landing) and blended by
        // speed, so a walk still glides and a plant still lands cleanly.
        //
        // The apex is anchored on `hip_lat` — the lateral line this leg's own
        // hip sits above — NOT on the (possibly widened) plant line. A tucked
        // swing foot is close to its hip, so the hip→ankle vector is short; any
        // lateral offset in it is then amplified by thigh_length/|hip→ankle|
        // (~4× at mid-swing) into a knee that bows out sideways. Aiming the tuck
        // straight under the hip keeps that lateral at zero, which is what keeps
        // the knee driving forward instead of splaying.
        let intensity = arc * norm_speed;
        let blend = (intensity * tuning.knee_drive).clamp(0.0, 1.0);
        let apex_x = ground.x + hip_lat.x + forward.x * tuning.knee_forward;
        let apex_z = ground.z + hip_lat.z + forward.z * tuning.knee_forward;
        let px = lerp(glide_x, apex_x, blend);
        let pz = lerp(glide_z, apex_z, blend);
        let lift = arc * tuning.foot_lift + intensity * tuning.knee_height;
        foot.target = Vec3::new(px, ground_y + lift, pz);
    }
}
