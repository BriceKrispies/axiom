//! The animation module's identity vocabulary — the value-type ids the
//! [`crate::AnimationApi`] facade hands back and accepts.
//!
//! All three are opaque, deterministic `u64` newtypes. [`SkeletonId`] and
//! [`ClipId`] are handles into an [`crate::AnimationApi`]'s registry, allocated
//! monotonically as skeletons/clips are created and never reused. [`BoneId`] is
//! a stable bone index *within* a skeleton (bone `0` is the first bone added).
//! None depend on pointer addresses or randomness, so the same sequence of
//! `create_*` calls always yields the same ids in the same order — safe to
//! store in snapshots and replay logs.

/// A handle to a skeleton registered in an [`crate::AnimationApi`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SkeletonId(u64);

impl SkeletonId {
    /// Construct from a raw index.
    pub const fn from_raw(raw: u64) -> Self {
        SkeletonId(raw)
    }

    /// The underlying raw index.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// A handle to an animation clip registered in an [`crate::AnimationApi`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClipId(u64);

impl ClipId {
    /// Construct from a raw index.
    pub const fn from_raw(raw: u64) -> Self {
        ClipId(raw)
    }

    /// The underlying raw index.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// A stable bone index within a single skeleton. Bone `0` is the first bone
/// added; a child bone always has a larger index than its parent (the parent
/// must exist first), which is what makes model-space resolution a single
/// forward pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BoneId(u64);

impl BoneId {
    /// Construct from a raw index.
    pub const fn from_raw(raw: u64) -> Self {
        BoneId(raw)
    }

    /// The underlying raw index.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_order_numerically() {
        assert_eq!(SkeletonId::from_raw(3).raw(), 3);
        assert_eq!(ClipId::from_raw(9).raw(), 9);
        assert_eq!(BoneId::from_raw(0).raw(), 0);
        assert!(BoneId::from_raw(1) < BoneId::from_raw(2));
        assert_eq!(SkeletonId::from_raw(4), SkeletonId::from_raw(4));
        assert_ne!(ClipId::from_raw(1), ClipId::from_raw(2));
    }
}
