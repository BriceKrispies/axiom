//! A single bone: its parent link and its rest (bind-pose) local transform.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};
use axiom_math::Transform;

use crate::ids::BoneId;

/// One bone in a [`crate::Skeleton`]. A bone carries its parent (`None` for a
/// root) and its **rest** local transform — the default local pose used wherever
/// a clip does not animate this bone. A bone holds no world/model transform;
/// model-space is derived on demand from a [`crate::Pose`] against the skeleton.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bone {
    parent: Option<BoneId>,
    rest: Transform,
}

impl Bone {
    /// A root bone (no parent) with the given rest local transform.
    pub const fn root(rest: Transform) -> Self {
        Bone { parent: None, rest }
    }

    /// A child bone parented to `parent` with the given rest local transform.
    pub const fn child(parent: BoneId, rest: Transform) -> Self {
        Bone {
            parent: Some(parent),
            rest,
        }
    }

    /// This bone's parent, or `None` if it is a root.
    pub const fn parent(self) -> Option<BoneId> {
        self.parent
    }

    /// This bone's rest (bind-pose) local transform.
    pub const fn rest(self) -> Transform {
        self.rest
    }

    /// Append this bone's bytes: a one-byte parent tag (`0` root, `1` child +
    /// the parent's `u64` id) followed by the rest transform.
    pub(crate) fn write_to(self, writer: &mut BinaryWriter) {
        writer.write_u8(u8::from(self.parent.is_some()));
        self.parent.iter().for_each(|parent| writer.write_u64(parent.raw()));
        self.rest.write_to(writer);
    }

    /// Read a bone written by [`Bone::write_to`].
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Bone> {
        reader
            .read_tagged(&[
                |_| Ok(None),
                |r| r.read_u64().map(|raw| Some(BoneId::from_raw(raw))),
            ])
            .and_then(|parent| Transform::read_from(reader).map(|rest| Bone { parent, rest }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::Vec3;

    #[test]
    fn root_has_no_parent_and_keeps_rest() {
        let rest = Transform::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let bone = Bone::root(rest);
        assert_eq!(bone.parent(), None);
        assert_eq!(bone.rest(), rest);
    }

    #[test]
    fn child_records_parent_and_rest() {
        let rest = Transform::from_translation(Vec3::new(0.0, 1.0, 0.0));
        let bone = Bone::child(BoneId::from_raw(0), rest);
        assert_eq!(bone.parent(), Some(BoneId::from_raw(0)));
        assert_eq!(bone.rest(), rest);
    }

    fn round_trip(bone: Bone) -> Bone {
        let mut w = BinaryWriter::new();
        bone.write_to(&mut w);
        let bytes = w.into_bytes();
        Bone::read_from(&mut BinaryReader::new(&bytes)).unwrap()
    }

    #[test]
    fn root_and_child_bones_round_trip() {
        let root = Bone::root(Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)));
        let child = Bone::child(BoneId::from_raw(4), Transform::from_translation(Vec3::new(0.0, -1.0, 0.5)));
        assert_eq!(round_trip(root), root);
        assert_eq!(round_trip(child), child);
    }

    #[test]
    fn truncated_bytes_fail_to_decode() {
        let bone = Bone::child(BoneId::from_raw(2), Transform::IDENTITY);
        let mut w = BinaryWriter::new();
        bone.write_to(&mut w);
        let bytes = w.into_bytes();
        assert!(Bone::read_from(&mut BinaryReader::new(&bytes[..bytes.len() - 1])).is_err());
    }
}
