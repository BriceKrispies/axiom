//! The rig topology: a flat list of named bones, each naming its parent.
//!
//! A [`SkeletonDefinition`] is ordinary editable data — a `Vec<BoneDefinition>`
//! the caller can grow, rename, or re-parent. The one invariant the animation
//! core relies on is a **parent-before-child ordering**: every bone's parent
//! index is strictly less than the bone's own index. That single rule makes the
//! skeleton acyclic, root-anchored, and safe to evaluate with a single forward
//! pass of forward kinematics — no per-frame cycle check, no recursion.

/// One bone in a [`SkeletonDefinition`]: a name plus the index of its parent
/// bone, or `None` for a root bone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoneDefinition {
    /// Human-readable bone name, e.g. `"right_foot"`. Used for lookup and for
    /// targeting clip tracks/events; not required to be unique by the type, but
    /// [`SkeletonDefinition::bone_index`] returns the first match.
    pub name: String,
    /// Index into [`SkeletonDefinition::bones`] of this bone's parent, or `None`
    /// if this bone is a root. A valid skeleton keeps `parent < own index`.
    pub parent: Option<usize>,
}

impl BoneDefinition {
    /// A root bone (no parent) with the given name.
    pub fn root(name: &str) -> Self {
        Self {
            name: name.to_string(),
            parent: None,
        }
    }

    /// A child bone parented to `parent` (an index into the bone list).
    pub fn child(name: &str, parent: usize) -> Self {
        Self {
            name: name.to_string(),
            parent: Some(parent),
        }
    }
}

/// Why a [`SkeletonDefinition`] failed validation. Returned by
/// [`SkeletonDefinition::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkeletonError {
    /// The skeleton has no bones at all.
    Empty,
    /// No bone is a root (every bone declares a parent, so there is a cycle).
    NoRoot,
    /// Bone `bone` names a parent that is not strictly earlier in the list —
    /// an out-of-range, forward, or self reference. This is the check that keeps
    /// the topology acyclic and single-pass evaluable.
    BadParent {
        /// Index of the offending child bone.
        bone: usize,
    },
}

/// A rig's bone topology: a flat, parent-before-child ordered list of bones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkeletonDefinition {
    /// The bones, in an order where every parent precedes its children.
    pub bones: Vec<BoneDefinition>,
}

impl SkeletonDefinition {
    /// Wrap a bone list. Does not validate — call [`Self::validate`].
    pub fn new(bones: Vec<BoneDefinition>) -> Self {
        Self { bones }
    }

    /// Number of bones in the rig.
    pub fn bone_count(&self) -> usize {
        self.bones.len()
    }

    /// Index of the first bone named `name`, or `None` if there is none.
    pub fn bone_index(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name == name)
    }

    /// Validate the topology: the rig must be non-empty, have at least one root,
    /// and every bone's parent must be strictly earlier in the list. Returns the
    /// first violation found.
    pub fn validate(&self) -> Result<(), SkeletonError> {
        let non_empty = (!self.bones.is_empty())
            .then_some(())
            .ok_or(SkeletonError::Empty);
        let has_root = self
            .bones
            .iter()
            .any(|b| b.parent.is_none())
            .then_some(())
            .ok_or(SkeletonError::NoRoot);
        let parents_ok = self.bones.iter().enumerate().try_for_each(|(i, b)| {
            b.parent.map_or(Ok(()), |p| {
                (p < i)
                    .then_some(())
                    .ok_or(SkeletonError::BadParent { bone: i })
            })
        });
        non_empty.and(has_root).and(parents_ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain() -> SkeletonDefinition {
        SkeletonDefinition::new(vec![
            BoneDefinition::root("root"),
            BoneDefinition::child("pelvis", 0),
            BoneDefinition::child("spine", 1),
        ])
    }

    #[test]
    fn valid_chain_validates_and_looks_up_bones() {
        let s = chain();
        assert_eq!(s.validate(), Ok(()));
        assert_eq!(s.bone_count(), 3);
        assert_eq!(s.bone_index("spine"), Some(2));
        assert_eq!(s.bone_index("missing"), None);
    }

    #[test]
    fn empty_skeleton_is_empty_error() {
        assert_eq!(
            SkeletonDefinition::new(vec![]).validate(),
            Err(SkeletonError::Empty)
        );
    }

    #[test]
    fn rootless_skeleton_is_no_root_error() {
        // Two bones each pointing at the other's slot: no `None` parent anywhere.
        let s = SkeletonDefinition::new(vec![
            BoneDefinition::child("a", 1),
            BoneDefinition::child("b", 0),
        ]);
        assert_eq!(s.validate(), Err(SkeletonError::NoRoot));
    }

    #[test]
    fn out_of_range_parent_is_bad_parent() {
        let s = SkeletonDefinition::new(vec![
            BoneDefinition::root("root"),
            BoneDefinition::child("x", 9),
        ]);
        assert_eq!(s.validate(), Err(SkeletonError::BadParent { bone: 1 }));
    }

    #[test]
    fn forward_reference_parent_is_bad_parent() {
        let s = SkeletonDefinition::new(vec![
            BoneDefinition::root("root"),
            BoneDefinition::child("child", 2),
            BoneDefinition::child("grandchild", 1),
        ]);
        assert_eq!(s.validate(), Err(SkeletonError::BadParent { bone: 1 }));
    }

    #[test]
    fn self_reference_parent_is_bad_parent() {
        let s = SkeletonDefinition::new(vec![
            BoneDefinition::root("root"),
            BoneDefinition::child("loop", 1),
        ]);
        assert_eq!(s.validate(), Err(SkeletonError::BadParent { bone: 1 }));
    }
}
