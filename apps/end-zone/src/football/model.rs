//! The football's visual model: a prolate silhouette (the engine's unit
//! sphere scaled along its long axis) plus a procedural lace ridge box.
//! Orientation is a pure function of the ball's simulation state — tucked
//! horizontal when carried or at rest, spiraling along the flight axis in the
//! air.

use axiom_math::{Quat, Transform, Vec3};

use super::state::{BallSim, BallState, BALL_VISUAL_SCALE};

/// The rotation taking the mesh's long axis (+Y) onto `axis`.
fn align_long_axis(axis: Vec3) -> Quat {
    let from = Vec3::UNIT_Y;
    let dot = from.dot(axis).clamp(-1.0, 1.0);
    if dot > 0.9999 {
        return Quat::IDENTITY;
    }
    if dot < -0.9999 {
        return Quat::from_axis_angle(Vec3::UNIT_X, core::f32::consts::PI)
            .unwrap_or(Quat::IDENTITY);
    }
    let cross = from.cross(axis);
    Quat::from_axis_angle(cross, dot.acos()).unwrap_or(Quat::IDENTITY)
}

/// The ball's world transform. `carrier_facing` orients a held/dead ball
/// (tucked horizontally along the carrier's facing); airborne balls align to
/// the flight axis and spiral around it.
pub fn ball_transform(ball: &BallSim, carrier_facing: Option<f32>) -> Transform {
    let rotation = match ball.state {
        BallState::Airborne { .. } => {
            let axis = if ball.vel.length() > 0.5 {
                ball.vel.normalize().unwrap_or(ball.flight_axis)
            } else {
                ball.flight_axis
            };
            align_long_axis(axis).multiply(Quat::from_euler_xyz(0.0, ball.spin_angle, 0.0))
        }
        BallState::Loose => {
            align_long_axis(Vec3::UNIT_Z).multiply(Quat::from_euler_xyz(0.0, ball.spin_angle, 0.0))
        }
        _ => {
            let facing = carrier_facing.unwrap_or(0.0);
            let forward = Vec3::new(facing.sin(), 0.0, facing.cos());
            align_long_axis(forward)
        }
    };
    Transform::new(ball.pos, rotation, BALL_VISUAL_SCALE)
}

/// The lace ridge: a thin white box floating on the ball's upper surface,
/// following the ball's rotation.
pub fn lace_transform(ball_world: &Transform) -> Transform {
    let local_offset = Vec3::new(0.0, 0.0, 0.52);
    let offset = ball_world.rotation.rotate(Vec3::new(
        local_offset.x * BALL_VISUAL_SCALE.x,
        local_offset.y * BALL_VISUAL_SCALE.y,
        local_offset.z * BALL_VISUAL_SCALE.z,
    ));
    Transform::new(
        ball_world.translation.add(offset),
        ball_world.rotation,
        Vec3::new(0.05, 0.30, 0.035),
    )
}
