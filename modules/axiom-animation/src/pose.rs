//! Poses: the rest pose the rig is authored in, and the per-frame joint state a
//! clip produces.
//!
//! Two representations meet here. A [`BindPose`] holds one
//! [`LocalBoneTransform`] per bone — the fixed offset/orientation/scale that
//! places each bone relative to its parent at rest. A [`Pose`] holds the
//! *animated* degree of freedom: a per-bone Euler rotation (a `Vec3` of radians,
//! `x`/`y`/`z`) layered on top of the bind rotation. Keeping the animated DOF as
//! Euler angles is deliberate — it is the representation joint limits clamp
//! (see the solver) and keyframes interpolate, and it converts to a composable
//! [`Quat`] only at the moment forward kinematics needs it.

use axiom_math::{Quat, Transform, Vec3};

use crate::skeleton::SkeletonDefinition;

/// A bone's transform relative to its parent: translation, rotation, and scale.
/// The bind pose is a list of these; it mirrors the math layer's [`Transform`]
/// but is the animation core's own vocabulary type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalBoneTransform {
    /// Offset from the parent bone, in the parent's space.
    pub translation: Vec3,
    /// Rotation relative to the parent.
    pub rotation: Quat,
    /// Per-axis scale.
    pub scale: Vec3,
}

impl LocalBoneTransform {
    /// A bone offset by `translation` with identity rotation and unit scale —
    /// the common case for a rest-pose skeleton whose joints sit at fixed
    /// offsets and whose animation lives entirely in the [`Pose`].
    pub const fn offset(translation: Vec3) -> Self {
        Self {
            translation,
            rotation: Quat::IDENTITY,
            scale: Vec3::new(1.0, 1.0, 1.0),
        }
    }

    /// Full constructor.
    pub const fn new(translation: Vec3, rotation: Quat, scale: Vec3) -> Self {
        Self {
            translation,
            rotation,
            scale,
        }
    }

    /// View this local bone transform as a math-layer [`Transform`].
    pub const fn to_transform(self) -> Transform {
        Transform::new(self.translation, self.rotation, self.scale)
    }
}

/// The rig's rest pose: one [`LocalBoneTransform`] per bone, index-aligned with
/// the skeleton's bones.
#[derive(Debug, Clone, PartialEq)]
pub struct BindPose {
    /// One local transform per bone.
    pub locals: Vec<LocalBoneTransform>,
}

impl BindPose {
    /// Wrap a per-bone local-transform list.
    pub fn new(locals: Vec<LocalBoneTransform>) -> Self {
        Self { locals }
    }

    /// Number of bones this bind pose describes.
    pub fn bone_count(&self) -> usize {
        self.locals.len()
    }
}

/// A sampled animation state: the per-bone Euler rotation (radians) layered on
/// top of the bind pose. Index-aligned with the skeleton's bones; a bone no clip
/// track touches carries a zero rotation.
#[derive(Debug, Clone, PartialEq)]
pub struct Pose {
    /// Per-bone Euler rotation in radians (`x`, `y`, `z`), applied on top of the
    /// bind rotation.
    pub joint_eulers: Vec<Vec3>,
}

impl Pose {
    /// Wrap a per-bone Euler-rotation list.
    pub fn new(joint_eulers: Vec<Vec3>) -> Self {
        Self { joint_eulers }
    }

    /// A rest pose: zero rotation on every one of `bone_count` bones.
    pub fn rest(bone_count: usize) -> Self {
        Self {
            joint_eulers: vec![Vec3::ZERO; bone_count],
        }
    }

    /// Number of bones this pose covers.
    pub fn bone_count(&self) -> usize {
        self.joint_eulers.len()
    }

    /// Combine this pose with a bind pose into the concrete per-bone local
    /// transforms: each bone keeps its bind translation/scale and composes its
    /// bind rotation with the animated Euler rotation. Zips over the shorter of
    /// the two lists.
    pub fn local_transforms(&self, bind: &BindPose) -> Vec<LocalBoneTransform> {
        bind.locals
            .iter()
            .zip(self.joint_eulers.iter())
            .map(|(local, &euler)| {
                LocalBoneTransform::new(
                    local.translation,
                    local
                        .rotation
                        .multiply(Quat::from_euler_xyz(euler.x, euler.y, euler.z)),
                    local.scale,
                )
            })
            .collect()
    }

    /// Forward kinematics: each bone's world transform, in bone order. Relies on
    /// the skeleton's parent-before-child invariant (see
    /// [`SkeletonDefinition::validate`]) so a single forward fold suffices — a
    /// bone's parent world transform is already in the accumulator when the bone
    /// is reached. Expects `skeleton`, `bind`, and this pose to share one bone
    /// count.
    ///
    /// [`SkeletonDefinition::validate`]: crate::SkeletonDefinition::validate
    pub fn world_transforms(
        &self,
        skeleton: &SkeletonDefinition,
        bind: &BindPose,
    ) -> Vec<Transform> {
        let locals = self.local_transforms(bind);
        skeleton.bones.iter().enumerate().fold(
            Vec::with_capacity(locals.len()),
            |mut acc, (i, bone)| {
                let local = locals[i].to_transform();
                let world = bone
                    .parent
                    .map_or(local, |p| Transform::combine(acc[p], local));
                acc.push(world);
                acc
            },
        )
    }

    /// Convenience over [`Self::world_transforms`]: just the world-space joint
    /// positions (each bone's world translation), the thing a debug view draws
    /// bone lines and joint markers from.
    pub fn world_joint_positions(
        &self,
        skeleton: &SkeletonDefinition,
        bind: &BindPose,
    ) -> Vec<Vec3> {
        self.world_transforms(skeleton, bind)
            .iter()
            .map(|t| t.translation)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skeleton::BoneDefinition;
    use axiom_math::ApproxEq;

    fn eps() -> axiom_math::Epsilon {
        axiom_math::Epsilon::new(1.0e-4).unwrap()
    }

    fn two_bone() -> (SkeletonDefinition, BindPose) {
        let skeleton = SkeletonDefinition::new(vec![
            BoneDefinition::root("root"),
            BoneDefinition::child("tip", 0),
        ]);
        let bind = BindPose::new(vec![
            LocalBoneTransform::offset(Vec3::new(0.0, 1.0, 0.0)),
            LocalBoneTransform::offset(Vec3::new(0.0, 1.0, 0.0)),
        ]);
        (skeleton, bind)
    }

    #[test]
    fn offset_and_new_and_to_transform_agree() {
        let t = LocalBoneTransform::offset(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(t.rotation, Quat::IDENTITY);
        assert_eq!(t.scale, Vec3::new(1.0, 1.0, 1.0));
        let m = t.to_transform();
        assert_eq!(m.translation, Vec3::new(1.0, 2.0, 3.0));
        let full = LocalBoneTransform::new(Vec3::ZERO, Quat::IDENTITY, Vec3::new(2.0, 2.0, 2.0));
        assert_eq!(full.scale, Vec3::new(2.0, 2.0, 2.0));
    }

    #[test]
    fn rest_pose_is_all_zero_and_reports_counts() {
        let (_, bind) = two_bone();
        assert_eq!(bind.bone_count(), 2);
        let pose = Pose::rest(2);
        assert_eq!(pose.bone_count(), 2);
        assert_eq!(pose.joint_eulers, vec![Vec3::ZERO, Vec3::ZERO]);
    }

    #[test]
    fn rest_world_positions_stack_offsets() {
        let (skeleton, bind) = two_bone();
        let pose = Pose::rest(2);
        let p = pose.world_joint_positions(&skeleton, &bind);
        assert!(p[0].approx_eq(&Vec3::new(0.0, 1.0, 0.0), eps()));
        // Child inherits root's world and adds its own local offset.
        assert!(p[1].approx_eq(&Vec3::new(0.0, 2.0, 0.0), eps()));
    }

    #[test]
    fn rotating_root_swings_child_around() {
        let (skeleton, bind) = two_bone();
        // Rotate the root 90° about +X: its child offset (0,1,0) swings to +Z-ish.
        let pose = Pose::new(vec![
            Vec3::new(std::f32::consts::FRAC_PI_2, 0.0, 0.0),
            Vec3::ZERO,
        ]);
        let worlds = pose.world_transforms(&skeleton, &bind);
        let child = worlds[1].translation;
        // Root stays at (0,1,0); child's (0,1,0) offset rotates about +X to (0,1,1).
        assert!(child.approx_eq(&Vec3::new(0.0, 1.0, 1.0), eps()));
    }

    #[test]
    fn local_transforms_layer_euler_on_bind() {
        let (_, bind) = two_bone();
        let pose = Pose::new(vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.5, 0.0, 0.0)]);
        let locals = pose.local_transforms(&bind);
        assert_eq!(locals.len(), 2);
        assert_eq!(locals[0].rotation, Quat::IDENTITY);
        assert!(locals[1]
            .rotation
            .approx_eq(&Quat::from_euler_xyz(0.5, 0.0, 0.0), eps()));
    }
}
