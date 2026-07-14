//! The rig: resolve one player's world-space part boxes for this tick.
//! Accumulates each part's joint chain (spec offsets + this tick's joint
//! rotations), places the chain under the body transform (sim ground position
//! + facing + fall pitch + squash), and bakes the box offsets through the
//! engine's [`FigureApi::posed_parts`].

use axiom_figure::{FigureApi, FigureDefinition, PosedPart};
use axiom_math::{Quat, Transform, Vec3};

use super::animation::JointPose;
use super::model::{FIGURE_CENTER_Y, PARTS, PART_COUNT};

/// The world body transform for a player: ground position raised to the
/// figure center, yaw from facing, pitch/roll from the pose, squash from the
/// presentation (0 = none, 1 = fully squashed).
pub fn body_transform(ground_pos: Vec3, facing: f32, pose: &JointPose, squash: f32) -> Transform {
    let squash = squash.clamp(0.0, 1.0);
    let scale = Vec3::new(
        1.0 + squash * 0.25,
        1.0 - squash * 0.32,
        1.0 + squash * 0.25,
    );
    let rotation = Quat::from_euler_xyz(0.0, facing, 0.0).multiply(Quat::from_euler_xyz(
        pose.root_pitch,
        0.0,
        pose.root_roll,
    ));
    Transform::new(
        Vec3::new(
            ground_pos.x,
            ground_pos.y + FIGURE_CENTER_Y + pose.root_lift,
            ground_pos.z,
        ),
        rotation,
        scale,
    )
}

/// Resolve every part to world space: joint chain under the body transform,
/// box offsets baked by the figure facade. Falls back to an empty list if the
/// figure/pose ever disagree on part count (they are both compile-time here).
pub fn world_parts(figure: &FigureDefinition, body: Transform, pose: &JointPose) -> Vec<PosedPart> {
    let mut locals: Vec<Transform> = Vec::with_capacity(PART_COUNT);
    for (index, spec) in PARTS.iter().enumerate() {
        let local = Transform::new(spec.offset, pose.joints[index], Vec3::ONE);
        let chained = match spec.parent {
            None => local,
            Some(parent) => Transform::combine(locals[parent as usize], local),
        };
        locals.push(chained);
    }
    let worlds: Vec<Transform> = locals
        .iter()
        .map(|local| Transform::combine(body, *local))
        .collect();
    FigureApi::new()
        .posed_parts(figure, &worlds)
        .unwrap_or_default()
}
