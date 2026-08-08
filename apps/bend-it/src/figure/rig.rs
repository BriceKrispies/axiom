//! The rig: turn one figure plus one pose into world-space boxes.
//!
//! Two steps, and only the first is this game's business. Deriving the **visual
//! body root** from the authoritative ground position — applying the pose's
//! bounded lift, lateral weight shift, lean and bank — is a decision about how
//! this game's players carry themselves. Walking the parent chain and baking the
//! box offsets is not: it is figure mechanism, and it belongs to the figure
//! module, which is where it now lives
//! ([`FigureApi::posed_parts_from_joints`]).
//!
//! The derivation is strictly one-way. `ground` and `facing` are taken by value
//! and never written back, so nothing downstream of here can reach the
//! simulation: the keeper's reach test, the ball's path and the kick timing all
//! keep using the gameplay root.

use axiom_figure::{FigureApi, FigureDefinition, PosedPart};
use axiom_math::{Quat, Transform, Vec3};

use super::model::FIGURE_CENTER_Y;
use super::pose::JointPose;

/// The visual body root for a figure standing at `ground` and facing `facing`
/// radians about Y.
pub fn body_transform(ground: Vec3, facing: f32, pose: &JointPose) -> Transform {
    let rotation = Quat::from_euler_xyz(0.0, facing, 0.0).multiply(Quat::from_euler_xyz(
        pose.root_pitch,
        0.0,
        pose.root_roll,
    ));
    // The weight shift runs along the facing-right axis, so it stays a sideways
    // sway of the body whichever way the figure is turned.
    let right = Vec3::new(facing.cos(), 0.0, -facing.sin());
    Transform::new(
        Vec3::new(
            ground.x + right.x * pose.root_lateral,
            ground.y + FIGURE_CENTER_Y + pose.root_lift,
            ground.z + right.z * pose.root_lateral,
        ),
        rotation,
        Vec3::ONE,
    )
}

/// Resolve every part to world space. Falls back to an empty list if the figure
/// and the pose ever disagree on part count (both are compile-time here).
pub fn world_parts(figure: &FigureDefinition, body: Transform, pose: &JointPose) -> Vec<PosedPart> {
    FigureApi::new()
        .posed_parts_from_joints(figure, body, &pose.joints)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::figure::model::{soccer_figure, HEAD, PART_COUNT};

    #[test]
    fn the_body_root_lifts_the_figure_onto_its_feet() {
        let body = body_transform(Vec3::new(2.0, 0.0, -3.0), 0.0, &JointPose::neutral());
        assert_eq!(
            body.translation,
            Vec3::new(2.0, FIGURE_CENTER_Y, -3.0),
            "the origin sits a body height above the soles"
        );
        assert_eq!(body.scale, Vec3::ONE);
    }

    #[test]
    fn the_pose_root_offsets_are_applied_in_the_figures_own_frame() {
        let mut pose = JointPose::neutral();
        pose.root_lateral = 1.0;
        pose.root_lift = 0.5;
        // Facing +Z (facing = 0): right is +X.
        let facing_z = body_transform(Vec3::ZERO, 0.0, &pose);
        assert!((facing_z.translation.x - 1.0).abs() < 1.0e-5);
        assert!((facing_z.translation.y - (FIGURE_CENTER_Y + 0.5)).abs() < 1.0e-5);
        // Turned a quarter turn, the same shift has moved to the other axis.
        let turned = body_transform(Vec3::ZERO, core::f32::consts::FRAC_PI_2, &pose);
        assert!(turned.translation.x.abs() < 1.0e-5);
        assert!((turned.translation.z + 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn a_posed_keeper_is_a_whole_body_standing_where_it_was_put() {
        use crate::figure::{keeper_frame, KeeperMotion};
        use crate::tuning::Tuning;
        let figure = soccer_figure();
        let frame = keeper_frame(
            KeeperMotion {
                hips: Vec3::new(0.0, 0.92, 0.42),
                lean: 0.0,
                extend: 0.0,
                height_bias: 0.0,
            },
            &Tuning::DEFAULT.keeper,
        );
        let parts = world_parts(
            &figure,
            body_transform(frame.ground, frame.facing, &frame.pose),
            &frame.pose,
        );
        assert_eq!(parts.len(), PART_COUNT, "every part is resolved");
        let lowest = parts
            .iter()
            .map(|p| p.transform.translation.y - p.box_size.y * 0.5)
            .fold(f32::INFINITY, f32::min);
        let highest = parts
            .iter()
            .map(|p| p.transform.translation.y + p.box_size.y * 0.5)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(lowest > -0.2 && lowest < 0.2, "the boots are on the turf: {lowest}");
        assert!(highest > 1.4, "the keeper is a whole body, not a stump: {highest}");
        parts.iter().for_each(|p| {
            let at = p.transform.translation;
            assert!(at.x.abs() < 0.8, "part strayed to x={}", at.x);
            assert!((at.z - 0.42).abs() < 0.6, "part strayed to z={}", at.z);
        });
    }

    #[test]
    fn a_pose_moves_the_parts_it_names_and_leaves_the_rest() {
        let figure = soccer_figure();
        let rest = world_parts(
            &figure,
            body_transform(Vec3::ZERO, 0.0, &JointPose::neutral()),
            &JointPose::neutral(),
        );
        let mut pose = JointPose::neutral();
        pose.joints[crate::figure::model::TORSO] = Quat::from_euler_xyz(0.0, 0.8, 0.0);
        let twisted = world_parts(&figure, body_transform(Vec3::ZERO, 0.0, &pose), &pose);
        assert_eq!(rest.len(), PART_COUNT);
        assert_eq!(twisted.len(), PART_COUNT);
        // The pelvis is above the twist and does not move; the head is below it
        // in the chain and does.
        assert_eq!(rest[0].transform.translation, twisted[0].transform.translation);
        assert_ne!(
            rest[HEAD].transform.rotation,
            twisted[HEAD].transform.rotation
        );
    }
}
