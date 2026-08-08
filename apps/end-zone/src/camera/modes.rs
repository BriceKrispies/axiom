//! Camera modes: each mode is a pure function from the presentation snapshot
//! and named tuning to a desired base pose (eye, look target, field of view).

use axiom::prelude::Vec3;

use crate::data::CameraTuning;
use crate::presentation::snapshot::PresentationSnapshot;

/// The seven directed camera modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraMode {
    FormationWide,
    QuarterbackFollow,
    BallCarrierFollow,
    PassFlight,
    CatchResolve,
    Impact,
    /// **The run game's shot.** A close third-person chase, tucked in behind and
    /// a little above the running back, looking downfield.
    ///
    /// It is not [`CameraMode::BallCarrierFollow`] with new numbers, and the
    /// difference is the whole point. The follow shot is a *broadcast* camera:
    /// it sits well back, bends hard toward the carrier's velocity, and frames
    /// him against the field — beautiful, and useless for the only four
    /// judgements this game asks you to make. This one is composed backwards
    /// from those judgements:
    ///
    /// * **read the man in front of you** — low and close, so a defender eight
    ///   yards out fills real screen space instead of being four pixels near the
    ///   horizon;
    /// * **find the gap** — the yaw is pinned near straight downfield rather
    ///   than to the runner's heading, so the blocking ahead stays in a stable
    ///   frame you can read a seam out of, and a juke moves the *runner* across
    ///   the picture rather than swinging the whole picture;
    /// * **judge a jump** — the eye rises with the leap, but only partly, so you
    ///   can see how much daylight is under you;
    /// * **feel the speed** — close, low, and slightly wide of centre, where the
    ///   turf detail streams past.
    RunbackChase,
}

/// A camera pose: eye, look-at target, vertical field of view in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraPose {
    pub eye: Vec3,
    pub target: Vec3,
    pub fov_degrees: f32,
}

impl CameraPose {
    pub fn lerp(a: CameraPose, b: CameraPose, t: f32) -> CameraPose {
        let t = t.clamp(0.0, 1.0);
        let mix = |x: f32, y: f32| x + (y - x) * t;
        CameraPose {
            eye: Vec3::new(
                mix(a.eye.x, b.eye.x),
                mix(a.eye.y, b.eye.y),
                mix(a.eye.z, b.eye.z),
            ),
            target: Vec3::new(
                mix(a.target.x, b.target.x),
                mix(a.target.y, b.target.y),
                mix(a.target.z, b.target.z),
            ),
            fov_degrees: mix(a.fov_degrees, b.fov_degrees),
        }
    }
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// The desired base pose for `mode` this tick. `impact_focus` is the point an
/// Impact camera emphasizes; `catch_blend` is the CatchResolve progress
/// `0..=1` from ball focus to catcher focus.
pub fn desired_pose(
    mode: CameraMode,
    snapshot: &PresentationSnapshot,
    tuning: &CameraTuning,
    impact_focus: Vec3,
    catch_blend: f32,
) -> CameraPose {
    let back = Vec3::new(0.0, 0.0, -snapshot.drive_sign);
    match mode {
        CameraMode::FormationWide => {
            let anchor = flat(snapshot.ball.pos);
            // A low, head-on broadcast shot directly behind the offense: the eye
            // sits on the field's long axis (no side offset), so the goalpost
            // stays dead-centred, and low enough that the huddle reads large in
            // the foreground while the aim looks straight downfield.
            CameraPose {
                eye: anchor
                    .add(back.mul_scalar(tuning.formation_distance))
                    .add(Vec3::new(0.0, tuning.formation_height, 0.0)),
                target: anchor
                    .add(back.mul_scalar(-6.0))
                    .add(Vec3::new(0.0, 1.6, 0.0)),
                fov_degrees: tuning.base_fov_degrees + 2.0,
            }
        }
        CameraMode::QuarterbackFollow => {
            let qb = snapshot.player(snapshot.quarterback);
            let anchor = flat(qb.pos);
            CameraPose {
                eye: anchor
                    .add(back.mul_scalar(tuning.follow_distance * 0.85))
                    .add(Vec3::new(0.0, tuning.follow_height, 0.0)),
                target: anchor
                    .add(back.mul_scalar(-7.0))
                    .add(Vec3::new(0.0, 1.5, 0.0)),
                fov_degrees: tuning.base_fov_degrees,
            }
        }
        CameraMode::BallCarrierFollow => follow_carrier(snapshot, tuning),
        CameraMode::PassFlight => {
            let ball = snapshot.ball.pos;
            let arrival = snapshot.flight.map(|f| f.target).unwrap_or(ball);
            let mid = ball.add(arrival).mul_scalar(0.5);
            let span = flat(arrival.subtract(ball)).length();
            let range = (span * 0.55 + 7.0).max(tuning.flight_framing_radius);
            // A side rail perpendicular to the flight line keeps both the
            // ball and the landing area in frame without hugging the ball.
            let dir = flat(arrival.subtract(ball));
            let side = if dir.length() > 0.5 {
                let d = dir.mul_scalar(1.0 / dir.length());
                Vec3::new(d.z, 0.0, -d.x)
            } else {
                Vec3::new(1.0, 0.0, 0.0)
            };
            let side = side.mul_scalar(if side.x >= 0.0 { 1.0 } else { -1.0 });
            CameraPose {
                eye: mid
                    .add(side.mul_scalar(range))
                    .add(Vec3::new(0.0, range * 0.45 + 2.0, 0.0)),
                target: mid,
                fov_degrees: tuning.base_fov_degrees - 4.0,
            }
        }
        CameraMode::CatchResolve => {
            let ball_focus = CameraPose {
                eye: snapshot
                    .ball
                    .pos
                    .add(back.mul_scalar(tuning.follow_distance))
                    .add(Vec3::new(0.0, tuning.follow_height * 0.9, 0.0)),
                target: snapshot.ball.pos,
                fov_degrees: tuning.base_fov_degrees - 2.0,
            };
            let carrier_focus = follow_carrier(snapshot, tuning);
            CameraPose::lerp(ball_focus, carrier_focus, catch_blend)
        }
        CameraMode::Impact => CameraPose {
            eye: impact_focus
                .add(back.mul_scalar(6.5))
                .add(Vec3::new(2.2, 2.6, 0.0)),
            target: impact_focus.add(Vec3::new(0.0, 0.9, 0.0)),
            fov_degrees: tuning.base_fov_degrees - 8.0,
        },
        CameraMode::RunbackChase => runback_chase(snapshot, tuning),
    }
}

/// The chase shot behind the running back.
///
/// The yaw is a **clamped bend** off straight-downfield rather than a follow of
/// the runner's heading, and that clamp ([`CameraTuning::chase_max_yaw_lag`]) is
/// what makes the camera survive a juke. A camera that tracked the heading would
/// whip through 40° in a fifth of a second every time the player cut — the exact
/// moment they most need to see where they are going. Bending only a little
/// keeps the world stable and lets the *runner* be the thing that moves, which
/// is both more readable and a better sell of the cut.
///
/// The eye follows the leap at [`CameraTuning::chase_height_follow`] of the
/// runner's height, never all of it: rising with him completely would hide the
/// very thing the jump is for, which is how much air is between him and the man
/// underneath.
fn runback_chase(snapshot: &PresentationSnapshot, tuning: &CameraTuning) -> CameraPose {
    let back = Vec3::new(0.0, 0.0, -snapshot.drive_sign);
    let runner = snapshot
        .attempt
        .and_then(|step| step.runback.back)
        .map(|id| snapshot.player(id));
    let (anchor, vel, height) = match runner {
        Some(view) => (flat(view.pos), view.vel, view.pos.y),
        None => (flat(snapshot.ball.pos), snapshot.ball.vel, 0.0),
    };
    let speed = flat(vel).length();
    let behind = match speed > 1.5 {
        true => {
            let v = flat(vel).mul_scalar(1.0 / speed);
            let drive_yaw = back.x.atan2(back.z);
            let vel_yaw = (-v.x).atan2(-v.z);
            let delta = wrap_pi(vel_yaw - drive_yaw);
            let yaw = drive_yaw + delta.clamp(-tuning.chase_max_yaw_lag, tuning.chase_max_yaw_lag);
            Vec3::new(yaw.sin(), 0.0, yaw.cos())
        }
        false => back,
    };
    let rise = height * tuning.chase_height_follow;
    CameraPose {
        eye: anchor
            .add(behind.mul_scalar(tuning.chase_distance))
            .add(Vec3::new(0.0, tuning.chase_height + rise, 0.0)),
        // Aimed past the runner, down the field: the frame's centre of interest
        // is the traffic he is running into, not the back of his helmet.
        target: anchor
            .add(behind.mul_scalar(-tuning.chase_look_ahead))
            .add(Vec3::new(0.0, 1.35 + rise * 0.5, 0.0)),
        fov_degrees: tuning.base_fov_degrees + tuning.chase_fov_widen,
    }
}

/// Shortest signed angle into `-PI..=PI`.
fn wrap_pi(mut delta: f32) -> f32 {
    while delta > core::f32::consts::PI {
        delta -= core::f32::consts::TAU;
    }
    while delta < -core::f32::consts::PI {
        delta += core::f32::consts::TAU;
    }
    delta
}

/// Behind-and-above follow of the current carrier (falls back to the ball
/// when possession is empty) with velocity look-ahead and a yaw-lag clamp so
/// hard cuts by the carrier do not whip the camera.
fn follow_carrier(snapshot: &PresentationSnapshot, tuning: &CameraTuning) -> CameraPose {
    let back = Vec3::new(0.0, 0.0, -snapshot.drive_sign);
    let (anchor, vel, facing) = match snapshot.carrier() {
        Some(carrier) => (flat(carrier.pos), carrier.vel, carrier.facing),
        None => (flat(snapshot.ball.pos), snapshot.ball.vel, 0.0),
    };
    // Follow direction: primarily "behind the drive", bent toward the
    // carrier's velocity but clamped to the tuned yaw lag.
    let speed = flat(vel).length();
    let behind = if speed > 1.0 {
        let v = flat(vel).mul_scalar(1.0 / speed);
        let drive_yaw = back.x.atan2(back.z);
        let vel_yaw = (-v.x).atan2(-v.z);
        let mut delta = vel_yaw - drive_yaw;
        while delta > core::f32::consts::PI {
            delta -= core::f32::consts::TAU;
        }
        while delta < -core::f32::consts::PI {
            delta += core::f32::consts::TAU;
        }
        let yaw = drive_yaw + delta.clamp(-tuning.max_yaw_lag, tuning.max_yaw_lag);
        Vec3::new(yaw.sin(), 0.0, yaw.cos())
    } else {
        let _ = facing;
        back
    };
    let look_ahead = flat(vel).mul_scalar(tuning.look_ahead);
    CameraPose {
        eye: anchor
            .add(behind.mul_scalar(tuning.follow_distance))
            .add(Vec3::new(0.0, tuning.follow_height, 0.0)),
        target: anchor
            .add(look_ahead)
            .add(behind.mul_scalar(-4.0))
            .add(Vec3::new(0.0, 1.4, 0.0)),
        fov_degrees: tuning.base_fov_degrees,
    }
}
