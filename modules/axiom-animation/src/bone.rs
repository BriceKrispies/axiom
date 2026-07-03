//! A single bone: its parent link and its rest (bind-pose) local transform.

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
}
