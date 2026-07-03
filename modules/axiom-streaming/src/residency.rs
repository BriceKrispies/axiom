//! The residency ring facade: the resident-coordinate set and the deterministic
//! load/unload delta it computes as a focus point moves.

use std::collections::BTreeSet;

use crate::ids::{ChunkCoord, ResidencyDelta};

/// Tracks which integer lattice coordinates are resident and, as a focus point
/// moves, computes the deterministic set of coordinates to **load** and to
/// **unload**.
///
/// The ring is payload-agnostic: it never sees the data a coordinate carries. A
/// caller turns [`ResidencyDelta::load`] coordinates into payloads and
/// [`ResidencyDelta::unload`] coordinates into teardown. Coordinates the caller
/// marks *dirty* (via the `is_dirty` predicate — e.g. a player-edited chunk) are
/// never unloaded, so authored state survives an eviction pass.
///
/// The resident set is a [`BTreeSet`], so every emitted delta is ordered
/// deterministically — set iteration order never leaks into the load/unload
/// vectors, which is what keeps a replay byte-identical.
#[derive(Debug, Default)]
pub struct Residency {
    resident: BTreeSet<ChunkCoord>,
}

impl Residency {
    /// An empty residency — nothing resident yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many coordinates are currently resident.
    pub fn len(&self) -> usize {
        self.resident.len()
    }

    /// Whether nothing is resident.
    pub fn is_empty(&self) -> bool {
        self.resident.is_empty()
    }

    /// Whether `coord` is currently resident.
    pub fn contains(&self, coord: ChunkCoord) -> bool {
        self.resident.contains(&coord)
    }

    /// Every currently-resident coordinate, in the set's deterministic sorted
    /// order. This is the snapshot a renderer iterates each frame to decide which
    /// *loaded* chunks to cull and draw (the load/unload delta reports only what
    /// *changed*; visibility needs the whole resident set).
    pub fn resident_coords(&self) -> Vec<ChunkCoord> {
        self.resident.iter().copied().collect()
    }

    /// Advance the ring to a new focus and return the delta.
    ///
    /// **Loads** every coordinate in the `[-radius, radius]²` square around
    /// `center` that is not already resident, in row-major `(z, x)` scan order.
    /// **Unloads** every resident coordinate lying outside the larger
    /// `[-(radius + margin), radius + margin]²` keep square, *unless* `is_dirty`
    /// retains it, in the resident set's sorted order. `margin` is the hysteresis
    /// band: a coordinate must fall outside the wider keep radius before it is
    /// evicted, so a focus jittering across a chunk boundary does not thrash. The
    /// resident set is mutated to match the returned delta.
    pub fn apply(
        &mut self,
        center: ChunkCoord,
        radius: i32,
        margin: i32,
        is_dirty: impl Fn(ChunkCoord) -> bool,
    ) -> ResidencyDelta {
        let load: Vec<ChunkCoord> = (-radius..=radius)
            .flat_map(|dz| {
                (-radius..=radius).map(move |dx| ChunkCoord::new(center.x + dx, center.z + dz))
            })
            .filter(|c| !self.resident.contains(c))
            .collect();
        let keep = radius + margin;
        let unload: Vec<ChunkCoord> = self
            .resident
            .iter()
            .copied()
            .filter(|c| outside(*c, center, keep) & !is_dirty(*c))
            .collect();
        load.iter().for_each(|c| {
            self.resident.insert(*c);
        });
        unload.iter().for_each(|c| {
            self.resident.remove(c);
        });
        ResidencyDelta { load, unload }
    }
}

/// Whether `c` lies outside the `[-keep, keep]²` square around `center`.
/// Branchless: the two Chebyshev-axis tests are combined with a bitwise `|`
/// (both operands pure and always safe to evaluate).
fn outside(c: ChunkCoord, center: ChunkCoord, keep: i32) -> bool {
    ((c.x - center.x).abs() > keep) | ((c.z - center.z).abs() > keep)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: ChunkCoord = ChunkCoord { x: 0, z: 0 };
    /// Nothing is ever dirty.
    fn clean(_: ChunkCoord) -> bool {
        false
    }

    #[test]
    fn new_ring_is_empty() {
        let r = Residency::new();
        assert!(r.is_empty());
        assert_eq!(r.len(), 0);
        assert!(!r.contains(ORIGIN));
    }

    #[test]
    fn radius_zero_loads_only_the_center() {
        let mut r = Residency::new();
        let delta = r.apply(ORIGIN, 0, 0, clean);
        assert_eq!(delta.load, vec![ORIGIN]);
        assert!(delta.unload.is_empty());
        assert_eq!(r.len(), 1);
        assert!(r.contains(ORIGIN));
    }

    #[test]
    fn ring_counts_are_the_square_of_the_diameter() {
        // (2r+1)^2 coordinates: r=1 -> 9, r=2 -> 25.
        let mut r1 = Residency::new();
        assert_eq!(r1.apply(ORIGIN, 1, 1, clean).load.len(), 9);
        let mut r2 = Residency::new();
        assert_eq!(r2.apply(ORIGIN, 2, 1, clean).load.len(), 25);
    }

    #[test]
    fn load_is_row_major_and_deterministic() {
        let mut r = Residency::new();
        let delta = r.apply(ORIGIN, 1, 1, clean);
        // (z, x) scan order: z outer from -1, x inner from -1.
        assert_eq!(
            delta.load,
            vec![
                ChunkCoord::new(-1, -1),
                ChunkCoord::new(0, -1),
                ChunkCoord::new(1, -1),
                ChunkCoord::new(-1, 0),
                ChunkCoord::new(0, 0),
                ChunkCoord::new(1, 0),
                ChunkCoord::new(-1, 1),
                ChunkCoord::new(0, 1),
                ChunkCoord::new(1, 1),
            ]
        );
    }

    #[test]
    fn re_applying_the_same_focus_loads_nothing() {
        let mut r = Residency::new();
        r.apply(ORIGIN, 2, 1, clean);
        let again = r.apply(ORIGIN, 2, 1, clean);
        assert!(again.load.is_empty());
        assert!(again.unload.is_empty());
        assert_eq!(r.len(), 25);
    }

    #[test]
    fn moving_the_focus_loads_only_newly_entered_coords() {
        let mut r = Residency::new();
        r.apply(ORIGIN, 1, 0, clean); // [-1,1]^2 = 9 resident
                                      // Shift east by one: the new column x=2 (z in -1..=1) enters.
        let delta = r.apply(ChunkCoord::new(1, 0), 1, 0, clean);
        assert_eq!(
            delta.load,
            vec![
                ChunkCoord::new(2, -1),
                ChunkCoord::new(2, 0),
                ChunkCoord::new(2, 1),
            ]
        );
    }

    #[test]
    fn moving_the_focus_unloads_coords_beyond_the_keep_square() {
        let mut r = Residency::new();
        r.apply(ORIGIN, 1, 0, clean); // keep radius 1 (margin 0)
                                      // Shift east by one with margin 0: column x=-1 falls outside keep=1.
        let delta = r.apply(ChunkCoord::new(1, 0), 1, 0, clean);
        assert_eq!(
            delta.unload,
            vec![
                ChunkCoord::new(-1, -1),
                ChunkCoord::new(-1, 0),
                ChunkCoord::new(-1, 1),
            ]
        );
        assert!(!r.contains(ChunkCoord::new(-1, 0)));
    }

    #[test]
    fn margin_is_hysteresis_a_coord_within_it_is_kept() {
        let mut r = Residency::new();
        r.apply(ORIGIN, 1, 1, clean); // keep radius = 1 + 1 = 2
                                      // Shift east by one: column x=-1 is now Chebyshev-distance 2 from the new
                                      // center (1,0) — still inside keep=2, so nothing unloads.
        let delta = r.apply(ChunkCoord::new(1, 0), 1, 1, clean);
        assert!(delta.unload.is_empty());
        assert!(r.contains(ChunkCoord::new(-1, 0)));
    }

    #[test]
    fn unload_triggers_on_either_axis() {
        // A far coordinate on the z axis unloads too (covers the `|`'s z arm),
        // and one on the x axis (its x arm).
        let mut r = Residency::new();
        r.apply(ORIGIN, 2, 0, clean);
        let delta = r.apply(ChunkCoord::new(0, 3), 0, 0, clean); // keep=0 around (0,3)
                                                                 // Everything from the old ring is now > 0 away on some axis → all unload.
        assert_eq!(delta.unload.len(), 25);
        assert!(delta.unload.contains(&ChunkCoord::new(2, 0))); // far on x
        assert!(delta.unload.contains(&ChunkCoord::new(-2, 2))); // far on z
    }

    #[test]
    fn dirty_coords_are_never_unloaded() {
        let mut r = Residency::new();
        r.apply(ORIGIN, 1, 0, clean);
        // (-1, 0) would be evicted by the eastward move, but it is dirty.
        let dirty_edit = ChunkCoord::new(-1, 0);
        let delta = r.apply(ChunkCoord::new(1, 0), 1, 0, |c| c == dirty_edit);
        assert!(!delta.unload.contains(&dirty_edit));
        assert!(r.contains(dirty_edit));
        // Its clean neighbours in the same far column still unload.
        assert!(delta.unload.contains(&ChunkCoord::new(-1, 1)));
    }

    #[test]
    fn a_preserved_dirty_coord_is_not_reloaded_on_return() {
        let mut r = Residency::new();
        r.apply(ORIGIN, 1, 0, clean);
        let dirty_edit = ChunkCoord::new(-1, 0);
        r.apply(ChunkCoord::new(1, 0), 1, 0, |c| c == dirty_edit); // kept resident
                                                                   // Returning to the origin must NOT reload the preserved coord (still
                                                                   // resident ⇒ its authored payload survives untouched).
        let back = r.apply(ORIGIN, 1, 0, clean);
        assert!(!back.load.contains(&dirty_edit));
    }

    #[test]
    fn residency_is_debuggable() {
        let text = format!("{:?}", Residency::new());
        assert!(text.contains("Residency"));
    }

    #[test]
    fn resident_coords_snapshots_the_loaded_set_in_sorted_order() {
        let mut r = Residency::new();
        assert!(r.resident_coords().is_empty());
        r.apply(ORIGIN, 1, 0, clean); // 9 chunks resident
        let coords = r.resident_coords();
        assert_eq!(coords.len(), 9);
        // BTreeSet order: the min corner sorts first, the max corner last.
        assert_eq!(coords[0], ChunkCoord::new(-1, -1));
        assert_eq!(coords[8], ChunkCoord::new(1, 1));
        let mut sorted = coords.clone();
        sorted.sort();
        assert_eq!(coords, sorted);
    }
}
