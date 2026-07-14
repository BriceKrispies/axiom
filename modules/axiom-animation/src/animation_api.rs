//! The single public facade for the animation module.

use axiom_kernel::{BinaryReader, BinaryWriter, Ratio, Tick};
use axiom_math::{Mat4, Transform, Vec3};

use crate::animation_error::AnimationError;
use crate::animation_result::AnimationResult;
use crate::blend::blend_poses;
use crate::clip::AnimationClip;
use crate::ids::{BoneId, ClipId, SkeletonId};
use crate::joint_limit::JointLimit;
use crate::keyframe::Keyframe;
use crate::pose::{ModelPose, Pose};
use crate::skeleton::Skeleton;

/// The deterministic skeletal-animation facade — the only behavioral type in
/// the module. Skeletons and clips are registered here and referred to by
/// [`SkeletonId`] / [`ClipId`]; poses are produced by [`AnimationApi::sample`]
/// and [`AnimationApi::blend`] and resolved to model space by
/// [`AnimationApi::resolve_model`]. Every scalar crosses the boundary as a
/// value type ([`Tick`], [`Ratio`], [`Transform`]) — never a naked float — and
/// every fallible call returns an [`AnimationError`] rather than panicking.
#[derive(Debug, Default)]
pub struct AnimationApi {
    skeletons: Vec<Skeleton>,
    clips: Vec<AnimationClip>,
}

impl AnimationApi {
    /// A registry with no skeletons or clips.
    pub fn new() -> Self {
        AnimationApi {
            skeletons: Vec::new(),
            clips: Vec::new(),
        }
    }

    /// Register a new empty skeleton and return its handle.
    pub fn create_skeleton(&mut self) -> SkeletonId {
        let id = SkeletonId::from_raw(self.skeletons.len() as u64);
        self.skeletons.push(Skeleton::new());
        id
    }

    /// Add a root bone (no parent) with rest local `rest` to `skeleton`.
    pub fn add_root_bone(
        &mut self,
        skeleton: SkeletonId,
        rest: Transform,
    ) -> AnimationResult<BoneId> {
        self.skeleton_mut(skeleton).map(|s| s.push_root(rest))
    }

    /// Add a child bone parented to `parent` with rest local `rest` to
    /// `skeleton`. Fails if the skeleton or the parent bone does not exist.
    pub fn add_child_bone(
        &mut self,
        skeleton: SkeletonId,
        parent: BoneId,
        rest: Transform,
    ) -> AnimationResult<BoneId> {
        self.skeleton_mut(skeleton)
            .and_then(|s| s.add_child(parent, rest))
    }

    /// The number of bones in `skeleton`.
    pub fn bone_count(&self, skeleton: SkeletonId) -> AnimationResult<usize> {
        self.skeleton(skeleton).map(Skeleton::bone_count)
    }

    /// Register a new empty clip and return its handle.
    pub fn create_clip(&mut self) -> ClipId {
        let id = ClipId::from_raw(self.clips.len() as u64);
        self.clips.push(AnimationClip::new());
        id
    }

    /// Add a keyframe track for `bone` to `clip`. Each `(Tick, Transform)` pair
    /// is a keyframe; the times must be strictly increasing and non-empty.
    pub fn add_track(
        &mut self,
        clip: ClipId,
        bone: BoneId,
        keyframes: &[(Tick, Transform)],
    ) -> AnimationResult<()> {
        let keys: Vec<Keyframe> = keyframes
            .iter()
            .map(|&(time, transform)| Keyframe::new(time, transform))
            .collect();
        self.clip_mut(clip).and_then(|c| c.add_track(bone, keys))
    }

    /// Attach an opaque-coded event to `clip` at `at`. The `code` is a
    /// game-defined `u32`; the mechanism carries it without interpreting it.
    pub fn add_event(&mut self, clip: ClipId, at: Tick, code: u32) -> AnimationResult<()> {
        self.clip_mut(clip).map(|c| c.add_event(at, code))
    }

    /// Attach an opaque-coded phase spanning `[start, end)` to `clip`.
    pub fn add_phase(
        &mut self,
        clip: ClipId,
        start: Tick,
        end: Tick,
        code: u32,
    ) -> AnimationResult<()> {
        self.clip_mut(clip).map(|c| c.add_phase(start, end, code))
    }

    /// The codes of every event on `clip` that fires exactly at `tick`.
    pub fn events_at(&self, clip: ClipId, tick: Tick) -> AnimationResult<Vec<u32>> {
        self.clip(clip).map(|c| c.events_at(tick))
    }

    /// The code of the phase on `clip` covering `tick`, if any.
    pub fn phase_at(&self, clip: ClipId, tick: Tick) -> AnimationResult<Option<u32>> {
        self.clip(clip).map(|c| c.phase_at(tick))
    }

    /// The rest (bind) pose of `skeleton` — each bone at its rest local.
    pub fn rest_pose(&self, skeleton: SkeletonId) -> AnimationResult<Pose> {
        self.skeleton(skeleton).map(Pose::rest)
    }

    /// Sample `clip` at `tick` against `skeleton`, producing a full pose.
    pub fn sample(&self, skeleton: SkeletonId, clip: ClipId, tick: Tick) -> AnimationResult<Pose> {
        self.clip(clip)
            .and_then(|c| self.skeleton(skeleton).and_then(|s| c.sample(s, tick)))
    }

    /// Blend two poses at `blend` (clamped to `[0, 1]`). The poses must cover
    /// the same number of bones.
    pub fn blend(&self, a: &Pose, b: &Pose, blend: Ratio) -> AnimationResult<Pose> {
        blend_poses(a, b, blend.get().clamp(0.0, 1.0))
    }

    /// Resolve `pose` to model space against `skeleton`.
    pub fn resolve_model(&self, skeleton: SkeletonId, pose: &Pose) -> AnimationResult<ModelPose> {
        self.skeleton(skeleton).and_then(|s| pose.to_model(s))
    }

    /// The **inverse bind matrices** of `skeleton` at `bind_pose`: for each bone,
    /// the inverse of its model-space (world) matrix in the bind pose. A mesh is
    /// baked once with its vertices in bind-pose model space; multiplying a vertex
    /// by its bone's inverse bind matrix moves it into that bone's local frame, so
    /// [`Self::joint_matrices`] can re-place it under any later pose (the basis of
    /// linear blend skinning). Computed once, at bind time. A bone whose bind
    /// matrix is singular (e.g. a zero-scale bone) contributes the identity.
    pub fn inverse_binds(
        &self,
        skeleton: SkeletonId,
        bind_pose: &Pose,
    ) -> AnimationResult<Vec<Mat4>> {
        self.resolve_model(skeleton, bind_pose).map(|model| {
            model
                .world_matrices()
                .into_iter()
                .map(|m| m.inverse().unwrap_or(Mat4::IDENTITY))
                .collect()
        })
    }

    /// The per-bone **joint matrices** for `pose`: `world(bone) * inverse_bind(bone)`.
    /// This is the skinning palette the vertex shader blends by weight — each maps
    /// a bind-pose vertex bound to that bone into the posed model space.
    /// `inverse_binds` must be [`Self::inverse_binds`] for the same skeleton (its
    /// length must equal the bone count), else `PoseLengthMismatch`.
    pub fn joint_matrices(
        &self,
        skeleton: SkeletonId,
        pose: &Pose,
        inverse_binds: &[Mat4],
    ) -> AnimationResult<Vec<Mat4>> {
        self.resolve_model(skeleton, pose).and_then(|model| {
            (model.bone_count() == inverse_binds.len())
                .then(|| {
                    model
                        .world_matrices()
                        .into_iter()
                        .zip(inverse_binds.iter())
                        .map(|(world, &inv)| world.multiply(inv))
                        .collect()
                })
                .ok_or_else(|| {
                    AnimationError::pose_length_mismatch(
                        "inverse-bind count does not match the skeleton",
                    )
                })
        })
    }

    /// Build a validated anatomical joint limit for `bone` with per-axis Euler
    /// `min`/`max` bounds (radians). The returned value is handed back to
    /// [`AnimationApi::clamp_pose`] / [`AnimationApi::is_pose_legal`]. Fails if
    /// any `min` axis exceeds its `max`.
    pub fn joint_limit(bone: BoneId, min: Vec3, max: Vec3) -> AnimationResult<JointLimit> {
        JointLimit::new(bone, min, max)
    }

    /// Clamp every bone in `pose` that has a matching entry in `limits` back
    /// into its anatomical range; bones without a limit pass through unchanged.
    pub fn clamp_pose(&self, limits: &[JointLimit], pose: &Pose) -> Pose {
        let locals = (0..pose.bone_count())
            .map(|i| {
                let bone = BoneId::from_raw(i as u64);
                let local = pose.local(bone).unwrap_or(Transform::IDENTITY);
                limits
                    .iter()
                    .find(|l| l.bone() == bone)
                    .map(|l| l.clamp_transform(local))
                    .unwrap_or(local)
            })
            .collect();
        Pose::from_locals(locals)
    }

    /// Whether every limit in `limits` is already satisfied by `pose` (a limit
    /// whose bone is absent from the pose is vacuously satisfied).
    pub fn is_pose_legal(&self, limits: &[JointLimit], pose: &Pose) -> bool {
        limits.iter().all(|l| {
            pose.local(l.bone())
                .map(|t| l.contains(t.rotation))
                .unwrap_or(true)
        })
    }

    /// Encode `skeleton` to a portable byte buffer that
    /// [`AnimationApi::deserialize_skeleton`] can reload into any registry.
    /// Fails with `SkeletonNotFound` for an unknown id.
    pub fn serialize_skeleton(&self, skeleton: SkeletonId) -> AnimationResult<Vec<u8>> {
        self.skeleton(skeleton).map(|s| {
            let mut writer = BinaryWriter::new();
            s.write_to(&mut writer);
            writer.into_bytes()
        })
    }

    /// Decode a skeleton previously produced by
    /// [`AnimationApi::serialize_skeleton`], registering it and returning its
    /// fresh id. Fails with `MalformedData` if the bytes cannot be decoded.
    pub fn deserialize_skeleton(&mut self, bytes: &[u8]) -> AnimationResult<SkeletonId> {
        Skeleton::read_from(&mut BinaryReader::new(bytes))
            .map_err(|_| AnimationError::malformed_data("could not decode skeleton bytes"))
            .map(|skeleton| {
                let id = SkeletonId::from_raw(self.skeletons.len() as u64);
                self.skeletons.push(skeleton);
                id
            })
    }

    /// Encode `clip` to a portable byte buffer that
    /// [`AnimationApi::deserialize_clip`] can reload. Fails with `ClipNotFound`
    /// for an unknown id.
    pub fn serialize_clip(&self, clip: ClipId) -> AnimationResult<Vec<u8>> {
        self.clip(clip).map(|c| {
            let mut writer = BinaryWriter::new();
            c.write_to(&mut writer);
            writer.into_bytes()
        })
    }

    /// Decode a clip previously produced by [`AnimationApi::serialize_clip`],
    /// registering it and returning its fresh id. Fails with `MalformedData` if
    /// the bytes cannot be decoded.
    pub fn deserialize_clip(&mut self, bytes: &[u8]) -> AnimationResult<ClipId> {
        AnimationClip::read_from(&mut BinaryReader::new(bytes))
            .map_err(|_| AnimationError::malformed_data("could not decode clip bytes"))
            .map(|clip| {
                let id = ClipId::from_raw(self.clips.len() as u64);
                self.clips.push(clip);
                id
            })
    }

    /// Immutable skeleton lookup.
    fn skeleton(&self, id: SkeletonId) -> AnimationResult<&Skeleton> {
        self.skeletons
            .get(id.raw() as usize)
            .ok_or_else(|| AnimationError::skeleton_not_found("no skeleton with that id"))
    }

    /// Mutable skeleton lookup.
    fn skeleton_mut(&mut self, id: SkeletonId) -> AnimationResult<&mut Skeleton> {
        self.skeletons
            .get_mut(id.raw() as usize)
            .ok_or_else(|| AnimationError::skeleton_not_found("no skeleton with that id"))
    }

    /// Immutable clip lookup.
    fn clip(&self, id: ClipId) -> AnimationResult<&AnimationClip> {
        self.clips
            .get(id.raw() as usize)
            .ok_or_else(|| AnimationError::clip_not_found("no clip with that id"))
    }

    /// Mutable clip lookup.
    fn clip_mut(&mut self, id: ClipId) -> AnimationResult<&mut AnimationClip> {
        self.clips
            .get_mut(id.raw() as usize)
            .ok_or_else(|| AnimationError::clip_not_found("no clip with that id"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation_error_code::AnimationErrorCode;
    use axiom_math::{ApproxEq, Epsilon, Quat, Vec3};

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-5).unwrap()
    }

    fn tx(x: f32) -> Transform {
        Transform::from_translation(Vec3::new(x, 0.0, 0.0))
    }

    /// A one-root, one-child skeleton with a clip animating the child from
    /// x=0 (tick 0) to x=10 (tick 10).
    fn rig() -> (AnimationApi, SkeletonId, ClipId) {
        let mut api = AnimationApi::new();
        let skel = api.create_skeleton();
        let root = api.add_root_bone(skel, tx(0.0)).unwrap();
        let child = api.add_child_bone(skel, root, Transform::IDENTITY).unwrap();
        let clip = api.create_clip();
        api.add_track(
            clip,
            child,
            &[(Tick::new(0), tx(0.0)), (Tick::new(10), tx(10.0))],
        )
        .unwrap();
        (api, skel, clip)
    }

    #[test]
    fn new_and_default_are_equivalent_empty_registries() {
        let a = AnimationApi::new();
        let b = AnimationApi::default();
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
    }

    #[test]
    fn ids_are_allocated_monotonically() {
        let mut api = AnimationApi::new();
        assert_eq!(api.create_skeleton(), SkeletonId::from_raw(0));
        assert_eq!(api.create_skeleton(), SkeletonId::from_raw(1));
        assert_eq!(api.create_clip(), ClipId::from_raw(0));
        assert_eq!(api.create_clip(), ClipId::from_raw(1));
    }

    #[test]
    fn bone_count_reports_added_bones() {
        let (api, skel, _) = rig();
        assert_eq!(api.bone_count(skel).unwrap(), 2);
    }

    #[test]
    fn skeleton_serialize_deserialize_round_trips_through_the_facade() {
        let (mut api, skel, _) = rig();
        let bytes = api.serialize_skeleton(skel).unwrap();
        let reloaded = api.deserialize_skeleton(&bytes).unwrap();
        assert_ne!(reloaded, skel);
        assert_eq!(
            api.bone_count(reloaded).unwrap(),
            api.bone_count(skel).unwrap()
        );
    }

    #[test]
    fn clip_serialize_deserialize_reproduces_the_same_sample() {
        let (mut api, skel, clip) = rig();
        let bytes = api.serialize_clip(clip).unwrap();
        let reloaded = api.deserialize_clip(&bytes).unwrap();
        let original = api
            .resolve_model(skel, &api.sample(skel, clip, Tick::new(7)).unwrap())
            .unwrap();
        let copy = api
            .resolve_model(skel, &api.sample(skel, reloaded, Tick::new(7)).unwrap())
            .unwrap();
        assert_eq!(
            original.position(BoneId::from_raw(1)),
            copy.position(BoneId::from_raw(1))
        );
    }

    fn mats_close(a: Mat4, b: Mat4) -> bool {
        a.as_cols_array()
            .iter()
            .zip(b.as_cols_array().iter())
            .all(|(x, y)| (x - y).abs() < 1.0e-4)
    }

    #[test]
    fn inverse_binds_invert_the_bind_world_matrices() {
        let (api, skel, _) = rig();
        let rest = api.rest_pose(skel).unwrap();
        let inv = api.inverse_binds(skel, &rest).unwrap();
        assert_eq!(inv.len(), 2);
        // world(bind) * inverse_bind == identity for each bone.
        let world = api.resolve_model(skel, &rest).unwrap().world_matrices();
        world
            .into_iter()
            .zip(inv)
            .for_each(|(w, i)| assert!(mats_close(w.multiply(i), Mat4::IDENTITY)));
    }

    #[test]
    fn a_singular_bind_bone_contributes_the_identity_inverse() {
        let mut api = AnimationApi::new();
        let skel = api.create_skeleton();
        // A zero-scale bone collapses its world matrix (non-invertible).
        api.add_root_bone(skel, Transform::from_scale(Vec3::ZERO))
            .unwrap();
        let rest = api.rest_pose(skel).unwrap();
        let inv = api.inverse_binds(skel, &rest).unwrap();
        assert_eq!(inv.len(), 1);
        assert!(mats_close(inv[0], Mat4::IDENTITY));
    }

    #[test]
    fn joint_matrices_at_the_bind_pose_are_identity() {
        let (api, skel, _) = rig();
        let rest = api.rest_pose(skel).unwrap();
        let inv = api.inverse_binds(skel, &rest).unwrap();
        let palette = api.joint_matrices(skel, &rest, &inv).unwrap();
        assert_eq!(palette.len(), 2);
        palette
            .into_iter()
            .for_each(|m| assert!(mats_close(m, Mat4::IDENTITY)));
    }

    #[test]
    fn joint_matrices_under_a_pose_move_a_bound_vertex_with_its_bone() {
        let (api, skel, clip) = rig();
        let rest = api.rest_pose(skel).unwrap();
        let inv = api.inverse_binds(skel, &rest).unwrap();
        // At tick 10 the child bone has translated to x=10 (bind was x=0).
        let posed = api.sample(skel, clip, Tick::new(10)).unwrap();
        let palette = api.joint_matrices(skel, &posed, &inv).unwrap();
        // A vertex at the child's bind position (origin), rigidly bound to the
        // child, follows the bone to x=10.
        let moved = palette[1].transform_point(Vec3::ZERO);
        assert!(moved.approx_eq(&Vec3::new(10.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn joint_matrices_reject_a_mismatched_inverse_bind_count() {
        let (api, skel, _) = rig();
        let rest = api.rest_pose(skel).unwrap();
        assert_eq!(
            api.joint_matrices(skel, &rest, &[]).unwrap_err().code(),
            AnimationErrorCode::PoseLengthMismatch
        );
    }

    #[test]
    fn serializing_unknown_ids_reports_not_found() {
        let api = AnimationApi::new();
        assert_eq!(
            api.serialize_skeleton(SkeletonId::from_raw(3))
                .unwrap_err()
                .code(),
            AnimationErrorCode::SkeletonNotFound
        );
        assert_eq!(
            api.serialize_clip(ClipId::from_raw(3)).unwrap_err().code(),
            AnimationErrorCode::ClipNotFound
        );
    }

    #[test]
    fn deserializing_garbage_reports_malformed_data() {
        let mut api = AnimationApi::new();
        assert_eq!(
            api.deserialize_skeleton(&[0xFF]).unwrap_err().code(),
            AnimationErrorCode::MalformedData
        );
        assert_eq!(
            api.deserialize_clip(&[0xFF]).unwrap_err().code(),
            AnimationErrorCode::MalformedData
        );
    }

    #[test]
    fn sampling_then_resolving_composes_the_hierarchy() {
        let (api, skel, clip) = rig();
        let pose = api.sample(skel, clip, Tick::new(10)).unwrap();
        let model = api.resolve_model(skel, &pose).unwrap();
        // Child local x=10 under a root at x=0 → model x=10.
        assert!(model
            .transform(BoneId::from_raw(1))
            .unwrap()
            .translation
            .approx_eq(&Vec3::new(10.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn sampling_is_replayable() {
        let (api, skel, clip) = rig();
        assert_eq!(
            api.sample(skel, clip, Tick::new(7)).unwrap(),
            api.sample(skel, clip, Tick::new(7)).unwrap()
        );
    }

    #[test]
    fn rest_pose_reads_bind_transforms() {
        let (api, skel, _) = rig();
        let rest = api.rest_pose(skel).unwrap();
        assert_eq!(rest.bone_count(), 2);
    }

    #[test]
    fn blend_of_two_sampled_poses_interpolates() {
        let (api, skel, clip) = rig();
        let start = api.sample(skel, clip, Tick::new(0)).unwrap();
        let end = api.sample(skel, clip, Tick::new(10)).unwrap();
        let mid = api.blend(&start, &end, Ratio::new(0.5).unwrap()).unwrap();
        assert!(mid
            .local(BoneId::from_raw(1))
            .unwrap()
            .translation
            .approx_eq(&Vec3::new(5.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn blend_factor_is_clamped_out_of_range() {
        let (api, skel, clip) = rig();
        let start = api.sample(skel, clip, Tick::new(0)).unwrap();
        let end = api.sample(skel, clip, Tick::new(10)).unwrap();
        // A factor above 1 clamps to the end pose.
        let over = api.blend(&start, &end, Ratio::new(4.0).unwrap()).unwrap();
        assert_eq!(over, end);
    }

    #[test]
    fn missing_skeleton_and_clip_ids_fail_deterministically() {
        let mut api = AnimationApi::new();
        let ghost_skel = SkeletonId::from_raw(99);
        let ghost_clip = ClipId::from_raw(99);
        assert_eq!(
            api.bone_count(ghost_skel).unwrap_err().code(),
            AnimationErrorCode::SkeletonNotFound
        );
        assert_eq!(
            api.rest_pose(ghost_skel).unwrap_err().code(),
            AnimationErrorCode::SkeletonNotFound
        );
        assert_eq!(
            api.add_root_bone(ghost_skel, tx(0.0)).unwrap_err().code(),
            AnimationErrorCode::SkeletonNotFound
        );
        assert_eq!(
            api.add_child_bone(ghost_skel, BoneId::from_raw(0), tx(0.0))
                .unwrap_err()
                .code(),
            AnimationErrorCode::SkeletonNotFound
        );
        let real_skel = api.create_skeleton();
        assert_eq!(
            api.add_track(ghost_clip, BoneId::from_raw(0), &[(Tick::new(0), tx(0.0))])
                .unwrap_err()
                .code(),
            AnimationErrorCode::ClipNotFound
        );
        assert_eq!(
            api.sample(real_skel, ghost_clip, Tick::new(0))
                .unwrap_err()
                .code(),
            AnimationErrorCode::ClipNotFound
        );
        let pose = api.rest_pose(real_skel).unwrap();
        assert_eq!(
            api.resolve_model(ghost_skel, &pose).unwrap_err().code(),
            AnimationErrorCode::SkeletonNotFound
        );
        // The event/phase methods reject a missing clip id too.
        assert_eq!(
            api.add_event(ghost_clip, Tick::new(0), 1)
                .unwrap_err()
                .code(),
            AnimationErrorCode::ClipNotFound
        );
        assert_eq!(
            api.add_phase(ghost_clip, Tick::new(0), Tick::new(1), 1)
                .unwrap_err()
                .code(),
            AnimationErrorCode::ClipNotFound
        );
        assert_eq!(
            api.events_at(ghost_clip, Tick::new(0)).unwrap_err().code(),
            AnimationErrorCode::ClipNotFound
        );
        assert_eq!(
            api.phase_at(ghost_clip, Tick::new(0)).unwrap_err().code(),
            AnimationErrorCode::ClipNotFound
        );
    }

    #[test]
    fn events_and_phases_round_trip_through_the_facade() {
        let mut api = AnimationApi::new();
        let clip = api.create_clip();
        api.add_event(clip, Tick::new(8), 42).unwrap();
        api.add_phase(clip, Tick::new(0), Tick::new(8), 3).unwrap();
        assert_eq!(api.events_at(clip, Tick::new(8)).unwrap(), vec![42]);
        assert_eq!(api.phase_at(clip, Tick::new(5)).unwrap(), Some(3));
        assert_eq!(api.phase_at(clip, Tick::new(8)).unwrap(), None);
    }

    #[test]
    fn joint_limit_rejects_inverted_bounds() {
        assert_eq!(
            AnimationApi::joint_limit(BoneId::from_raw(0), Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO)
                .unwrap_err()
                .code(),
            AnimationErrorCode::InvalidJointLimit
        );
    }

    #[test]
    fn clamp_pose_constrains_limited_bones_and_passes_others_through() {
        let api = AnimationApi::new();
        // Two-bone pose: bone 0 over-rotated on X, bone 1 left at identity.
        let pose = Pose::from_locals(vec![
            Transform::from_rotation(Quat::from_euler_xyz(1.0, 0.0, 0.0)),
            Transform::from_rotation(Quat::from_euler_xyz(0.2, 0.0, 0.0)),
        ]);
        // Limit ONLY bone 0's X axis to [0, 0.5]; also a limit for an absent
        // bone 9 (must be ignored). Bone 1 has no limit → unchanged.
        let limits = vec![
            AnimationApi::joint_limit(
                BoneId::from_raw(0),
                Vec3::new(0.0, -0.1, -0.1),
                Vec3::new(0.5, 0.1, 0.1),
            )
            .unwrap(),
            AnimationApi::joint_limit(BoneId::from_raw(9), Vec3::ZERO, Vec3::ZERO).unwrap(),
        ];
        assert!(!api.is_pose_legal(&limits, &pose));
        let clamped = api.clamp_pose(&limits, &pose);
        // Bone 0 clamped to x = 0.5; bone 1 untouched at x = 0.2.
        assert!(
            (clamped
                .local(BoneId::from_raw(0))
                .unwrap()
                .rotation
                .to_euler_xyz()
                .x
                - 0.5)
                .abs()
                <= 1.0e-3
        );
        assert!(clamped
            .local(BoneId::from_raw(1))
            .unwrap()
            .rotation
            .approx_eq(&Quat::from_euler_xyz(0.2, 0.0, 0.0), eps()));
        assert!(api.is_pose_legal(&limits, &clamped));
    }
}
