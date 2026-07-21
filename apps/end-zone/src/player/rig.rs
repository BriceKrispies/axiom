//! The rig: resolve one player's world-space part boxes for this tick.
//! Accumulates each part's joint chain (spec offsets + this tick's joint
//! rotations), places the chain under the body transform (sim ground position
//! + facing + fall pitch + squash), and bakes the box offsets through the
//! engine's [`FigureApi::posed_parts`].

use axiom_figure::{FigureApi, FigureDefinition, PosedPart};
use axiom_math::{Quat, Transform, Vec3};

use super::animation::JointPose;
use super::model::{FIGURE_CENTER_Y, PARTS, PART_COUNT};

/// The **visual body root** for a player: the cosmetic frame derived from the
/// authoritative gameplay root (`ground_pos` + `facing`, straight from the
/// simulation) by applying the pose's bounded visual offsets — vertical weight
/// transfer, lateral shift toward the stance leg, lean/bank — plus the
/// presentation squash (0 = none, 1 = fully squashed).
///
/// This derivation is strictly one-way. `ground_pos`/`facing` are taken by
/// value and never written back; nothing downstream of here can reach the
/// simulation. Movement, collision and tackling keep using the gameplay root.
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
    // Lateral weight shift runs along the facing-right axis, so it stays a
    // sideways sway of the body regardless of which way the player is running.
    let right = Vec3::new(facing.cos(), 0.0, -facing.sin());
    Transform::new(
        Vec3::new(
            ground_pos.x + right.x * pose.root_lateral,
            ground_pos.y + FIGURE_CENTER_Y + pose.root_lift,
            ground_pos.z + right.z * pose.root_lateral,
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
