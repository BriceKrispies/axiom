//! A typed, copyable handle to an asset held in an [`crate::prelude::Assets`]
//! collection.

use std::marker::PhantomData;

/// A stable handle to an asset of type `T`.
///
/// Identity is a 1-based slot id within the `Assets<T>` that minted it. The
/// trait impls are written by hand rather than derived so a `Handle<T>` is
/// `Copy`/`Eq`/`Debug` regardless of `T` — in particular `Handle<Material>` is
/// `Eq` even though `Material` (carrying `f32` colour) is not.
pub struct Handle<T> {
    id: u64,
    _marker: PhantomData<T>,
}

impl<T> Handle<T> {
    /// Mint a handle for a 1-based slot id. Crate-private: only an `Assets<T>`
    /// hands out real handles.
    pub(crate) const fn new(id: u64) -> Self {
        Handle {
            id,
            _marker: PhantomData,
        }
    }

    /// The 1-based slot id this handle refers to.
    pub const fn id(self) -> u64 {
        self.id
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Handle<T> {}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Eq for Handle<T> {}

impl<T> std::fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle").field("id", &self.id).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A throwaway marker type that is itself NOT Eq/Copy, proving Handle's impls
    // do not depend on T.
    #[derive(Debug)]
    struct NotCopyable(#[allow(dead_code)] String);

    #[test]
    fn id_round_trips() {
        let h: Handle<NotCopyable> = Handle::new(7);
        assert_eq!(h.id(), 7);
    }

    #[test]
    fn copy_and_clone_preserve_identity() {
        let h: Handle<NotCopyable> = Handle::new(3);
        let copied = h;
        let cloned = h.clone();
        assert_eq!(copied, cloned);
    }

    #[test]
    fn equality_is_by_id() {
        let a: Handle<NotCopyable> = Handle::new(1);
        let b: Handle<NotCopyable> = Handle::new(1);
        let c: Handle<NotCopyable> = Handle::new(2);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn debug_shows_the_id() {
        let h: Handle<NotCopyable> = Handle::new(42);
        assert!(format!("{h:?}").contains("42"));
    }
}
