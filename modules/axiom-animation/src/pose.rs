//! A pose (one local transform per bone) and its resolved model-space form.

use axiom_math::Transform;

use crate::animation_error::AnimationError;
use crate::animation_result::AnimationResult;
use crate::ids::BoneId;
use crate::skeleton::Skeleton;

/// A pose: exactly one **local** [`Transform`] per bone, indexed by [`BoneId`].
/// A pose is the output of sampling a clip or blending two poses, and the input
/// to model-space resolution. It carries no parent links itself — those live in
/// the [`Skeleton`] it is resolved against.
#[derive(Debug, Clone, PartialEq)]
pub struct Pose {
    locals: Vec<Transform>,
}

impl Pose {
    /// Build a pose directly from per-bone local transforms.
    pub(crate) fn from_locals(locals: Vec<Transform>) -> Pose {
        Pose { locals }
    }

    /// The rest (bind) pose of `skeleton`: each bone at its rest local
    /// transform. Deterministic and stable for a given skeleton.
    pub(crate) fn rest(skeleton: &Skeleton) -> Pose {
        let locals = (0..skeleton.bone_count())
            .map(|i| {
                skeleton
                    .bone(BoneId::from_raw(i as u64))
                    .map(|b| b.rest())
                    .unwrap_or(Transform::IDENTITY)
            })
            .collect();
        Pose { locals }
    }

    /// The number of bones this pose covers.
    pub fn bone_count(&self) -> usize {
        self.locals.len()
    }

    /// The local transform of `bone`, or `None` if `bone` is out of range.
    pub fn local(&self, bone: BoneId) -> Option<Transform> {
        self.locals.get(bone.raw() as usize).copied()
    }

    /// Internal slice view for blending.
    pub(crate) fn locals(&self) -> &[Transform] {
        &self.locals
    }

    /// Resolve this pose to model space against `skeleton`: each bone's model
    /// transform is its parent's model transform composed with its own local
    /// (roots use their local directly). Because a skeleton stores bones in
    /// parent-before-child order, this is a single forward pass. Fails with
    /// `PoseLengthMismatch` if the pose does not cover exactly the skeleton's
    /// bones.
    pub(crate) fn to_model(&self, skeleton: &Skeleton) -> AnimationResult<ModelPose> {
        (self.locals.len() == skeleton.bone_count())
            .then_some(())
            .ok_or_else(|| {
                AnimationError::pose_length_mismatch(
                    "pose bone count does not match the skeleton",
                )
            })
            .map(|()| {
                let models = self.locals.iter().enumerate().fold(
                    Vec::with_capacity(self.locals.len()),
                    |mut acc, (i, &local)| {
                        let model = skeleton
                            .bone(BoneId::from_raw(i as u64))
                            .and_then(|b| b.parent())
                            .map(|p| Transform::combine(acc[p.raw() as usize], local))
                            .unwrap_or(local);
                        acc.push(model);
                        acc
                    },
                );
                ModelPose { models }
            })
    }
}

/// A pose resolved to **model** space: one absolute [`Transform`] per bone,
/// each already composed through its parent chain. This is what an app reads
/// out to place renderables, and it never leaves the facade except as `Transform`
/// values.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelPose {
    models: Vec<Transform>,
}

impl ModelPose {
    /// The number of bones.
    pub fn bone_count(&self) -> usize {
        self.models.len()
    }

    /// The model-space transform of `bone`, or `None` if out of range.
    pub fn transform(&self, bone: BoneId) -> Option<Transform> {
        self.models.get(bone.raw() as usize).copied()
    }

    /// The model-space **position** of `bone` — the forward-kinematics joint
    /// location an app reads to place a renderable. `None` if out of range.
    pub fn position(&self, bone: BoneId) -> Option<axiom_math::Vec3> {
        self.transform(bone).map(|t| t.translation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation_error_code::AnimationErrorCode;
    use axiom_math::{ApproxEq, Epsilon, Vec3};

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-5).unwrap()
    }

    /// Root at x=1 with a child offset by x=2 in local space → child sits at
    /// x=3 in model space.
    fn parent_child() -> (Skeleton, Pose) {
        let mut skel = Skeleton::new();
        let root = skel.push_root(Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
        skel.add_child(root, Transform::from_translation(Vec3::new(2.0, 0.0, 0.0)))
            .unwrap();
        let pose = Pose::rest(&skel);
        (skel, pose)
    }

    #[test]
    fn rest_pose_is_stable_and_matches_bone_count() {
        let (skel, pose) = parent_child();
        assert_eq!(pose.bone_count(), 2);
        assert_eq!(Pose::rest(&skel), pose);
        assert!(pose
            .local(BoneId::from_raw(0))
            .unwrap()
            .translation
            .approx_eq(&Vec3::new(1.0, 0.0, 0.0), eps()));
        assert_eq!(pose.local(BoneId::from_raw(9)), None);
    }

    #[test]
    fn model_space_composes_parent_and_child() {
        let (skel, pose) = parent_child();
        let model = pose.to_model(&skel).unwrap();
        assert_eq!(model.bone_count(), 2);
        assert!(model
            .transform(BoneId::from_raw(0))
            .unwrap()
            .translation
            .approx_eq(&Vec3::new(1.0, 0.0, 0.0), eps()));
        assert!(model
            .transform(BoneId::from_raw(1))
            .unwrap()
            .translation
            .approx_eq(&Vec3::new(3.0, 0.0, 0.0), eps()));
        assert_eq!(model.transform(BoneId::from_raw(2)), None);
        // Joint positions are the model-space translations (forward kinematics).
        assert!(model
            .position(BoneId::from_raw(1))
            .unwrap()
            .approx_eq(&Vec3::new(3.0, 0.0, 0.0), eps()));
        assert_eq!(model.position(BoneId::from_raw(2)), None);
    }

    #[test]
    fn model_resolution_rejects_length_mismatch() {
        let (skel, _) = parent_child();
        let short = Pose::from_locals(vec![Transform::IDENTITY]);
        assert_eq!(
            short.to_model(&skel).unwrap_err().code(),
            AnimationErrorCode::PoseLengthMismatch
        );
    }

    #[test]
    fn empty_skeleton_has_empty_rest_pose() {
        let skel = Skeleton::new();
        let pose = Pose::rest(&skel);
        assert_eq!(pose.bone_count(), 0);
        assert_eq!(pose.to_model(&skel).unwrap().bone_count(), 0);
    }
}
