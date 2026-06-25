//! [`Address`] — a deterministic, serializable content address.
//!
//! An address is a hierarchical **`u64` key-path**: the root is the empty path,
//! and each child appends one `u64` segment. It is the stable name for
//! *what/where* content is generated — a chunk, a region, a content node — and
//! nothing more. It is deliberately **domain-free**: a segment is an opaque key,
//! not a coordinate carrying geometry semantics (callers encode their own space —
//! signed coordinates, multi-axis chunk indices — into segments). Geometry is
//! `math`'s job; meaning is a domain module's job; an address only *names* a site.
//!
//! Addresses are minted and operated on through [`crate::SpaceApi`]; this type is
//! the value vocabulary the facade hands back (the same shape as the kernel's id
//! primitives, or `ecs`'s `EntityHandle`).

/// A hierarchical content address: a path of `u64` key segments from the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    segments: Vec<u64>,
}

impl Address {
    /// Construct from owned segments. Crate-internal: callers mint addresses via
    /// [`crate::SpaceApi`] (`root` + `child`), so a path is always built by
    /// append (or read back from canonical bytes), never an arbitrary external
    /// vector.
    pub(crate) fn from_segments(segments: Vec<u64>) -> Self {
        Address { segments }
    }

    /// The address's segments, root-first.
    pub fn segments(&self) -> &[u64] {
        &self.segments
    }

    /// How deep the address is — the root has depth 0.
    pub fn depth(&self) -> usize {
        self.segments.len()
    }

    /// Whether this is the root address (the empty path).
    pub fn is_root(&self) -> bool {
        self.segments.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_is_empty_and_depth_grows_with_segments() {
        let root = Address::from_segments(vec![]);
        assert!(root.is_root());
        assert_eq!(root.depth(), 0);
        assert!(root.segments().is_empty());

        let deep = Address::from_segments(vec![3, 7, 9]);
        assert!(!deep.is_root());
        assert_eq!(deep.depth(), 3);
        assert_eq!(deep.segments(), &[3, 7, 9]);
    }

    #[test]
    fn equality_and_value_semantics() {
        let a = Address::from_segments(vec![1, 2]);
        let b = Address::from_segments(vec![1, 2]);
        let c = Address::from_segments(vec![1, 3]);
        assert_eq!(a, b);
        assert_eq!(a, a.clone());
        assert_ne!(a, c);
        assert!(!format!("{a:?}").is_empty());
    }
}
