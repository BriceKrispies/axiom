//! The pure value-type vocabulary the [`Residency`](crate::Residency) facade
//! traffics in: the integer lattice coordinate it addresses, and the load/unload
//! delta it hands back. These carry data, not engine state — they are the nouns
//! the ring returns and takes in (the `pub use ids::{…}` carve-out), the same
//! reason the `grid` module publishes `Grid`/`TileSpace` alongside `GridApi`.

/// An integer coordinate on the streamed 2-D lattice: a chunk address on the
/// horizontal plane the residency ring tracks.
///
/// It is deliberately plain — `Copy`, totally ordered, and hashable — so both
/// the ring (which keys a `BTreeSet` on it) and a caller's payload map (which
/// keys a `HashMap`/`BTreeMap` on it) name the same coordinate. The `x`/`z`
/// naming (not `x`/`y`) marks it as a ground-plane address; height/payload is the
/// caller's concern, never the ring's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ChunkCoord {
    pub x: i32,
    pub z: i32,
}

impl ChunkCoord {
    /// A coordinate at lattice cell `(x, z)`.
    pub fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }
}

/// The deterministic outcome of advancing the ring to a new focus: the
/// coordinates that must be **loaded** (newly resident, turn into payloads) and
/// the coordinates that must be **unloaded** (evicted, tear their payloads down).
///
/// Both vectors are ordered deterministically — `load` in the ring's row-major
/// `(z, x)` scan order, `unload` in the resident set's sorted order — so the
/// same focus move yields byte-identical deltas run-to-run and machine-to-
/// machine. Set iteration order never leaks into either vector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResidencyDelta {
    /// Coordinates entering residency this step, in row-major scan order.
    pub load: Vec<ChunkCoord>,
    /// Coordinates leaving residency this step, in sorted order.
    pub unload: Vec<ChunkCoord>,
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn new_default_and_fields() {
        let c = ChunkCoord::new(3, -2);
        assert_eq!(c.x, 3);
        assert_eq!(c.z, -2);
        // Default is the lattice origin.
        assert_eq!(ChunkCoord::default(), ChunkCoord::new(0, 0));
    }

    #[test]
    fn is_copy_and_clone_are_value_equal() {
        let c = ChunkCoord::new(1, 2);
        let copied = c; // Copy
        let cloned = c.clone(); // explicit Clone
        assert_eq!(copied, cloned);
        assert_eq!(c, ChunkCoord::new(1, 2));
        assert_ne!(c, ChunkCoord::new(2, 1));
    }

    #[test]
    fn total_order_is_lexicographic_x_then_z() {
        // PartialOrd (`<`) and Ord (`cmp`) agree, ordering by x then z.
        assert!(ChunkCoord::new(0, 0) < ChunkCoord::new(0, 1));
        assert!(ChunkCoord::new(0, 9) < ChunkCoord::new(1, 0));
        assert_eq!(
            ChunkCoord::new(1, 0).cmp(&ChunkCoord::new(1, 0)),
            std::cmp::Ordering::Equal
        );
    }

    #[test]
    fn is_hashable_as_a_set_key() {
        let mut set = HashSet::new();
        set.insert(ChunkCoord::new(4, 4));
        set.insert(ChunkCoord::new(4, 4));
        assert_eq!(set.len(), 1);
        assert!(set.contains(&ChunkCoord::new(4, 4)));
        assert!(!set.contains(&ChunkCoord::new(4, 5)));
    }

    #[test]
    fn debug_names_the_coordinate() {
        let text = format!("{:?}", ChunkCoord::new(7, -3));
        assert!(text.contains('7'));
        assert!(text.contains("-3"));
    }

    #[test]
    fn delta_is_debug_and_value_equal() {
        let a = ResidencyDelta {
            load: vec![ChunkCoord::new(0, 0)],
            unload: vec![ChunkCoord::new(1, 1)],
        };
        let b = a.clone();
        assert_eq!(a, b);
        assert_ne!(
            a,
            ResidencyDelta {
                load: vec![],
                unload: vec![]
            }
        );
        assert!(format!("{a:?}").contains("load"));
    }
}
