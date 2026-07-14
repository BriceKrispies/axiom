//! A skeleton: bones in parent-before-child insertion order.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};
use axiom_math::Transform;

use crate::animation_error::AnimationError;
use crate::animation_result::AnimationResult;
use crate::bone::Bone;
use crate::ids::BoneId;

/// A parented set of [`Bone`]s. Bones are stored in insertion order, and the
/// module's one construction rule is that a child is always added **after** its
/// parent — so every bone's parent has a strictly smaller [`BoneId`]. That
/// ordering is what lets [`crate::Pose`] resolve model-space in a single forward
/// pass with no sorting and no recursion.
///
/// A `Skeleton` is built through [`crate::AnimationApi`] (`create_skeleton` +
/// `add_root_bone` / `add_child_bone`); the type itself is reached only through
/// the facade.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Skeleton {
    bones: Vec<Bone>,
}

impl Skeleton {
    /// An empty skeleton.
    pub(crate) fn new() -> Self {
        Skeleton { bones: Vec::new() }
    }

    /// Append `bone`, returning its freshly-allocated id (its index).
    fn push(&mut self, bone: Bone) -> BoneId {
        let id = BoneId::from_raw(self.bones.len() as u64);
        self.bones.push(bone);
        id
    }

    /// Append a root bone (no parent) with the given rest local transform.
    pub(crate) fn push_root(&mut self, rest: Transform) -> BoneId {
        self.push(Bone::root(rest))
    }

    /// Append a child bone parented to `parent`. Fails with
    /// [`crate::animation_error_code::AnimationErrorCode::BoneNotFound`] if
    /// `parent` is not an already-added bone (which also guarantees the
    /// parent-before-child ordering the model pass relies on).
    pub(crate) fn add_child(&mut self, parent: BoneId, rest: Transform) -> AnimationResult<BoneId> {
        (parent.raw() < self.bones.len() as u64)
            .then(|| self.push(Bone::child(parent, rest)))
            .ok_or_else(|| AnimationError::bone_not_found("child bone parent index out of range"))
    }

    /// The number of bones.
    pub(crate) fn bone_count(&self) -> usize {
        self.bones.len()
    }

    /// The bone at `id`, or `None` if `id` is out of range.
    pub(crate) fn bone(&self, id: BoneId) -> Option<Bone> {
        self.bones.get(id.raw() as usize).copied()
    }

    /// Append the skeleton's bytes: a `u64` bone count then each bone in order.
    pub(crate) fn write_to(&self, writer: &mut BinaryWriter) {
        writer.write_u64(self.bones.len() as u64);
        self.bones.iter().for_each(|bone| bone.write_to(writer));
    }

    /// Read a skeleton written by [`Skeleton::write_to`].
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Skeleton> {
        reader.read_u64().and_then(|count| {
            (0..count)
                .try_fold(Vec::new(), |mut bones, _| {
                    Bone::read_from(reader).map(|bone| {
                        bones.push(bone);
                        bones
                    })
                })
                .map(|bones| Skeleton { bones })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation_error_code::AnimationErrorCode;
    use axiom_math::Vec3;

    fn t(x: f32) -> Transform {
        Transform::from_translation(Vec3::new(x, 0.0, 0.0))
    }

    #[test]
    fn bones_get_sequential_ids_in_insertion_order() {
        let mut skel = Skeleton::new();
        let root = skel.push_root(t(0.0));
        let child = skel.add_child(root, t(1.0)).unwrap();
        assert_eq!(root, BoneId::from_raw(0));
        assert_eq!(child, BoneId::from_raw(1));
        assert_eq!(skel.bone_count(), 2);
        assert_eq!(skel.bone(child).unwrap().parent(), Some(root));
        assert_eq!(skel.bone(root).unwrap().parent(), None);
    }

    #[test]
    fn out_of_range_parent_is_rejected() {
        let mut skel = Skeleton::new();
        let err = skel.add_child(BoneId::from_raw(7), t(1.0)).unwrap_err();
        assert_eq!(err.code(), AnimationErrorCode::BoneNotFound);
    }

    #[test]
    fn bone_lookup_out_of_range_is_none() {
        let skel = Skeleton::new();
        assert_eq!(skel.bone(BoneId::from_raw(0)), None);
    }

    #[test]
    fn skeleton_round_trips_through_bytes() {
        let mut skel = Skeleton::new();
        let root = skel.push_root(t(0.0));
        let a = skel.add_child(root, t(1.0)).unwrap();
        skel.add_child(a, t(2.0)).unwrap();
        let mut w = BinaryWriter::new();
        skel.write_to(&mut w);
        let bytes = w.into_bytes();
        let back = Skeleton::read_from(&mut BinaryReader::new(&bytes)).unwrap();
        assert_eq!(back, skel);
    }

    #[test]
    fn empty_skeleton_round_trips() {
        let skel = Skeleton::new();
        let mut w = BinaryWriter::new();
        skel.write_to(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            Skeleton::read_from(&mut BinaryReader::new(&bytes)).unwrap(),
            skel
        );
    }

    #[test]
    fn truncated_skeleton_bytes_fail() {
        assert!(Skeleton::read_from(&mut BinaryReader::new(&[])).is_err());
        let mut w = BinaryWriter::new();
        w.write_u64(3); // claims 3 bones, provides none
        let bytes = w.into_bytes();
        assert!(Skeleton::read_from(&mut BinaryReader::new(&bytes)).is_err());
    }
}
