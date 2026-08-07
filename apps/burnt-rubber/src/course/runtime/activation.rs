//! The **distance index**: constant-time "where in this sorted list does course
//! distance `s` start".
//!
//! Every per-frame question the runtime asks a [`CoursePlan`](super::CoursePlan)
//! is of the form *give me the first entry at or past this many metres*. Answered
//! by scanning a sorted list that is 100+ entries long, sixty times a second,
//! that is thousands of comparisons a second to learn something that changes
//! every few seconds. Answered by a bucket table it is one array read and a walk
//! of however many entries share a bucket.
//!
//! The table is a flat `Vec<u32>`: one entry per [`BUCKET_M`] of course, holding
//! the index of the first list entry whose distance reaches that bucket. It is
//! built once at compile time and is immutable, so the hot path allocates
//! nothing and cannot invalidate it.

/// How much course one index bucket covers (m).
///
/// A hundred metres is roughly a second and a half at racing speed and about
/// one traffic headway, so a bucket holds a handful of entries — small enough
/// that the walk after the lookup is trivial, large enough that a nine-kilometre
/// course needs ninety-odd `u32`s rather than thousands.
pub const BUCKET_M: f32 = 100.0;

/// A bucket index over a distance-sorted list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistanceIndex {
    /// `buckets[b]` is the first entry index at or past `b · BUCKET_M`.
    buckets: Vec<u32>,
    /// How many entries the indexed list holds.
    entries: usize,
}

impl DistanceIndex {
    /// Build an index over `distances`, which must be ascending.
    ///
    /// One forward pass over the list and one over the buckets, so building the
    /// index costs no more than reading the list it indexes.
    pub fn build(length_m: f32, distances: impl Iterator<Item = f32>) -> DistanceIndex {
        let distances: Vec<f32> = distances.collect();
        let buckets_needed = ((length_m.max(0.0) / BUCKET_M).ceil() as usize) + 2;
        let mut cursor = 0usize;
        let buckets: Vec<u32> = (0..buckets_needed)
            .map(|bucket| {
                let threshold = bucket as f32 * BUCKET_M;
                while (cursor < distances.len()) & (distances.get(cursor).copied().unwrap_or(f32::MAX) < threshold) {
                    cursor += 1;
                }
                cursor as u32
            })
            .collect();
        DistanceIndex {
            buckets,
            entries: distances.len(),
        }
    }

    /// A lower bound on the index of the first entry at or past `distance_m`.
    ///
    /// Never overshoots: the caller may walk forward from here and is guaranteed
    /// not to have skipped an entry.
    pub fn first_at(&self, distance_m: f32) -> usize {
        let bucket = (distance_m / BUCKET_M).floor();
        (bucket < 0.0)
            .then_some(0)
            .unwrap_or_else(|| {
                let bucket = (bucket as usize).min(self.buckets.len() - 1);
                self.buckets[bucket] as usize
            })
            .min(self.entries)
    }

    /// How many entries the indexed list holds.
    pub const fn entries(&self) -> usize {
        self.entries
    }

    /// How many buckets the table holds — the index's whole memory cost, in
    /// `u32`s.
    pub fn buckets(&self) -> usize {
        self.buckets.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(distances: &[f32], length_m: f32) -> DistanceIndex {
        DistanceIndex::build(length_m, distances.iter().copied())
    }

    /// The property the whole type has to have: the bucket answer is never past
    /// the true answer, so walking forward from it cannot skip an entry.
    fn brute(distances: &[f32], probe: f32) -> usize {
        distances
            .iter()
            .position(|d| *d >= probe)
            .unwrap_or(distances.len())
    }

    #[test]
    fn the_index_never_overshoots_the_true_answer() {
        let distances: Vec<f32> = (0..120).map(|i| 37.0 + i as f32 * 71.5).collect();
        let index = index(&distances, 9_000.0);
        for probe in (0..9_200).step_by(13) {
            let probe = probe as f32;
            let from = index.first_at(probe);
            let truth = brute(&distances, probe);
            assert!(
                from <= truth,
                "index said {from}, truth is {truth}, at {probe} m"
            );
            // And the walk from it lands exactly on the truth.
            let walked = from
                + distances[from..]
                    .iter()
                    .position(|d| *d >= probe)
                    .unwrap_or(distances.len() - from);
            assert_eq!(walked, truth);
        }
    }

    #[test]
    fn a_lookup_is_cheap_because_a_bucket_holds_only_a_few_entries() {
        // The shipping course's traffic density: about one car per 85 m, so a
        // 100 m bucket holds one or two.
        let distances: Vec<f32> = (0..105).map(|i| 300.0 + i as f32 * 85.0).collect();
        let index = index(&distances, 9_000.0);
        let worst = (0..90)
            .map(|b| {
                let from = index.first_at(b as f32 * BUCKET_M);
                let to = index.first_at((b + 1) as f32 * BUCKET_M);
                to.saturating_sub(from)
            })
            .max()
            .unwrap();
        assert!(worst <= 3, "a bucket held {worst} entries");
        assert!(index.buckets() < 100, "the whole table is {} u32s", index.buckets());
        assert_eq!(index.entries(), 105);
    }

    #[test]
    fn out_of_range_probes_clamp_at_both_ends() {
        let distances = [100.0f32, 200.0, 300.0];
        let index = index(&distances, 400.0);
        assert_eq!(index.first_at(-1_000.0), 0);
        assert_eq!(index.first_at(0.0), 0);
        assert_eq!(index.first_at(1.0e9), distances.len());
        assert_eq!(index.first_at(f32::MAX), distances.len());
    }

    #[test]
    fn an_empty_list_indexes_harmlessly() {
        let index = index(&[], 1_000.0);
        assert_eq!(index.entries(), 0);
        assert_eq!(index.first_at(0.0), 0);
        assert_eq!(index.first_at(500.0), 0);
        assert!(index.buckets() > 0);
    }

    #[test]
    fn entries_before_the_first_bucket_are_still_found() {
        let distances = [0.0f32, 5.0, 10.0];
        let index = index(&distances, 100.0);
        assert_eq!(index.first_at(0.0), 0);
        assert_eq!(index.first_at(6.0), 0, "the bucket must not overshoot");
        assert_eq!(brute(&distances, 6.0), 2);
    }

    #[test]
    fn repeated_distances_are_indexed_at_the_earliest_of_them() {
        let distances = [100.0f32, 100.0, 100.0, 400.0];
        let index = index(&distances, 500.0);
        assert_eq!(index.first_at(100.0), 0);
        assert_eq!(index.first_at(150.0), 0);
        assert_eq!(index.first_at(400.0), 3);
    }
}
