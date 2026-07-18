//! The module's identity vocabulary: the opaque handle the multi-humanoid
//! (crowd) surface of [`crate::PhysicalAnimationApi`] hands out and accepts. A
//! handle is an index into the controller's crowd of colliding humanoids that
//! share one physics world — the nouns the crowd methods traffic in, carrying no
//! behavior of their own.

/// A bound colliding humanoid in the shared world. Returned by
/// `bind_colliding_humanoid` and accepted by the crowd advance/readback methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HumanoidHandle(usize);

impl HumanoidHandle {
    /// Wrap a crowd index.
    pub(crate) fn new(index: usize) -> Self {
        HumanoidHandle(index)
    }

    /// The crowd index this handle addresses.
    pub(crate) fn index(self) -> usize {
        self.0
    }
}
