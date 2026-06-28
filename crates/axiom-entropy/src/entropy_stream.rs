//! [`EntropyStream`] — one keyed, reproducible entropy stream.
//!
//! A stream carries a stable **key** (its identity, independent of how far it has
//! been drawn) and a live [`axiom_kernel::DeterministicRng`] seeded from that key.
//! Two streams with the same key produce byte-identical sequences; [`Self::fork`]
//! derives an isolated sub-stream from the stable key (not the draw position), so
//! a fork is reproducible no matter how far the parent has advanced. It invents no
//! randomness — it routes the kernel's deterministic source.

use axiom_kernel::{DeterministicRng, Ratio, StableHash};

/// A deterministic, reproducible entropy stream keyed by a stable 64-bit key.
#[derive(Debug)]
pub struct EntropyStream {
    key: u64,
    rng: DeterministicRng,
}

impl EntropyStream {
    /// Seed a stream from its stable key. Crate-internal: streams are minted by
    /// [`crate::EntropyApi`] from a `(seed, address, version)` tuple, or forked.
    pub(crate) fn from_key(key: u64) -> Self {
        EntropyStream {
            key,
            rng: DeterministicRng::seeded(key),
        }
    }

    /// This stream's stable key — its identity, independent of draw position.
    pub fn key(&self) -> u64 {
        self.key
    }

    /// The next 64-bit value from the stream.
    pub fn next_u64(&mut self) -> u64 {
        self.rng.next_u64()
    }

    /// The next value uniformly in `[0, bound)` (`0` when `bound == 0`).
    pub fn next_bounded(&mut self, bound: u64) -> u64 {
        self.rng.next_bounded(bound)
    }

    /// An independent sub-stream keyed by `salt`, derived from this stream's
    /// stable key (not its current draw position) — so the same fork is
    /// reproducible no matter how far the parent has advanced. Generalizes the
    /// app-local `Rng::fork(salt)` from `apps/axiom-growth`.
    pub fn fork(&self, salt: u64) -> EntropyStream {
        EntropyStream::from_key(StableHash::of_words(&[self.key, salt]).raw())
    }

    /// A uniform draw in `[0, 1)` as a kernel [`Ratio`] — the unit interval, and
    /// the only floating value the stream produces. Built from the top 24 bits of
    /// a `u64` draw over `2^24`, so it is always finite and strictly below `1`.
    pub fn unit(&mut self) -> Ratio {
        let bits = self.rng.next_u64() >> 40;
        Ratio::new(bits as f32 / 16_777_216.0).expect("a 24-bit fraction over 2^24 is finite in [0, 1)")
    }

    /// A uniform integer in `[0, max_exclusive)` (yields `0` when
    /// `max_exclusive == 0`). The contract's `Rng::int`.
    pub fn int(&mut self, max_exclusive: u64) -> u64 {
        self.rng.next_bounded(max_exclusive)
    }

    /// `true` with probability `p`: the unit draw is compared against `p`, so
    /// `p <= 0` is never true and `p >= 1` is always true. The contract's
    /// `Rng::bool`.
    pub fn ratio_bool(&mut self, p: Ratio) -> bool {
        self.unit().get() < p.get()
    }

    /// A uniform index in `[0, len)` (yields `0` when `len == 0`; callers pick
    /// over a non-empty collection per the contract). The contract's `Rng::pick`
    /// chooses the value at this index.
    pub fn pick_index(&mut self, len: usize) -> usize {
        self.rng.next_bounded(len as u64) as usize
    }

    /// A cumulative-weight selection: index `i` is chosen with probability
    /// `weights[i] / Σ weights`, so a zero-weight entry is never chosen. Integer
    /// weights keep the choice exact and cross-machine identical. Empty or
    /// all-zero weights yield `0` (a degenerate input the projection guards).
    pub fn weighted_index(&mut self, weights: &[u64]) -> usize {
        let total = weights.iter().copied().fold(0u64, u64::saturating_add);
        let draw = self.rng.next_bounded(total);
        weights
            .iter()
            .scan(0u64, |cumulative, &weight| {
                *cumulative = cumulative.saturating_add(weight);
                Some(*cumulative)
            })
            .position(|cumulative| cumulative > draw)
            .unwrap_or(0)
    }

    /// In-place Fisher-Yates shuffle, deterministic for a given draw sequence.
    /// Expressed as a descending index fold (no control flow): each position is
    /// swapped with a uniformly chosen earlier-or-equal index.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        (1..items.len()).rev().for_each(|i| {
            let j = self.rng.next_bounded(i as u64 + 1) as usize;
            items.swap(i, j);
        });
    }

    /// An independent sub-stream named by `name`, reproducible across runs and
    /// platforms — a [`fork`](Self::fork) keyed by the stable hash of the name's
    /// bytes, exactly as a stream is keyed by an `Address`. The contract's
    /// `Rng::stream`.
    pub fn named(&self, name: &str) -> EntropyStream {
        self.fork(StableHash::of_bytes(name.as_bytes()).raw())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_key_yields_identical_sequence() {
        let mut a = EntropyStream::from_key(0xABCD);
        let mut b = EntropyStream::from_key(0xABCD);
        assert_eq!(a.key(), 0xABCD);
        for _ in 0..16 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn distinct_keys_diverge() {
        let mut a = EntropyStream::from_key(1);
        let mut b = EntropyStream::from_key(2);
        assert!((0..16).any(|_| a.next_u64() != b.next_u64()));
    }

    #[test]
    fn next_bounded_stays_in_range() {
        let mut s = EntropyStream::from_key(42);
        for _ in 0..256 {
            assert!(s.next_bounded(10) < 10);
        }
    }

    #[test]
    fn fork_is_isolated_and_stable_across_parent_draws() {
        let parent = EntropyStream::from_key(7);
        let mut from_fresh = parent.fork(3);

        // A parent advanced by some draws still forks to the same sub-stream:
        // fork keys off the stable key, not the draw position.
        let mut moved = EntropyStream::from_key(7);
        (0..5).for_each(|_| {
            moved.next_u64();
        });
        let mut from_moved = moved.fork(3);

        assert_eq!(from_fresh.key(), from_moved.key());
        for _ in 0..8 {
            assert_eq!(from_fresh.next_u64(), from_moved.next_u64());
        }

        // A different salt is an independent sub-stream.
        let mut other = parent.fork(4);
        assert!((0..16).any(|_| parent.fork(3).next_u64() != other.next_u64()));
    }

    #[test]
    fn stream_is_debug() {
        assert!(!format!("{:?}", EntropyStream::from_key(1)).is_empty());
    }

    fn ratio(value: f32) -> Ratio {
        Ratio::new(value).unwrap()
    }

    #[test]
    fn unit_is_in_range_and_reproducible() {
        let mut a = EntropyStream::from_key(99);
        let mut b = EntropyStream::from_key(99);
        (0..256).for_each(|_| {
            let v = a.unit().get();
            assert!((0.0..1.0).contains(&v));
            assert_eq!(v, b.unit().get());
        });
    }

    #[test]
    fn int_is_bounded_and_zero_bound_is_zero() {
        let mut s = EntropyStream::from_key(5);
        assert_eq!(s.int(0), 0);
        (0..256).for_each(|_| assert!(s.int(7) < 7));
    }

    #[test]
    fn ratio_bool_honours_certain_and_impossible() {
        let mut s = EntropyStream::from_key(11);
        // p = 1 is always true, p = 0 is never true — both edges exercised.
        assert!((0..64).all(|_| s.ratio_bool(ratio(1.0))));
        assert!((0..64).all(|_| !s.ratio_bool(ratio(0.0))));
        // p = 0.5 produces a mix (both arms of the comparison occur).
        let trues = (0..256).filter(|_| s.ratio_bool(ratio(0.5))).count();
        assert!(trues > 0);
        assert!(trues < 256);
    }

    #[test]
    fn pick_index_is_in_range_and_singleton_is_zero() {
        let mut s = EntropyStream::from_key(13);
        assert_eq!(s.pick_index(1), 0);
        (0..256).for_each(|_| assert!(s.pick_index(4) < 4));
    }

    #[test]
    fn weighted_index_favours_weight_and_never_picks_zero() {
        let mut s = EntropyStream::from_key(17);
        // Index 1 has all the weight; index 0 and 2 (weight 0) never chosen.
        let weights = [0, 5, 0];
        assert!((0..256).all(|_| s.weighted_index(&weights) == 1));
        // A spread of weights lands on every positive bucket over many draws.
        let mut counts = [0u32; 3];
        (0..3_000).for_each(|_| counts[s.weighted_index(&[1, 2, 3])] += 1);
        assert!(counts.iter().all(|&c| c > 0));
        assert!(counts[2] > counts[0]); // heavier bucket wins more often
    }

    #[test]
    fn weighted_index_degenerate_inputs_yield_zero() {
        let mut s = EntropyStream::from_key(19);
        assert_eq!(s.weighted_index(&[]), 0); // empty
        assert_eq!(s.weighted_index(&[0, 0, 0]), 0); // all-zero
    }

    #[test]
    fn shuffle_is_a_permutation_and_reproducible() {
        let golden: Vec<u32> = (0..32).collect();
        let mut a: Vec<u32> = golden.clone();
        let mut b: Vec<u32> = golden.clone();
        EntropyStream::from_key(23).shuffle(&mut a);
        EntropyStream::from_key(23).shuffle(&mut b);
        // Same key ⇒ identical ordering (deterministic).
        assert_eq!(a, b);
        // It actually reordered, and preserved the multiset.
        assert_ne!(a, golden);
        let mut sorted = a.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, golden);
    }

    #[test]
    fn shuffle_of_short_slices_is_a_noop() {
        let mut empty: [u32; 0] = [];
        EntropyStream::from_key(1).shuffle(&mut empty);
        let mut one = [42];
        EntropyStream::from_key(1).shuffle(&mut one);
        assert_eq!(one, [42]);
    }

    #[test]
    fn named_streams_are_distinct_and_reproducible() {
        let root = EntropyStream::from_key(101);
        // Distinct names ⇒ divergent streams.
        assert!((0..16).any(|_| root.named("loot").next_u64() != root.named("spawn").next_u64()));
        // Same name ⇒ identical stream, regardless of how far the parent drew.
        let from_fresh_key = root.named("loot").key();
        let mut moved = EntropyStream::from_key(101);
        (0..9).for_each(|_| {
            moved.next_u64();
        });
        assert_eq!(moved.named("loot").key(), from_fresh_key);
        // named(name) equals fork(hash(name)).
        assert_eq!(
            root.named("loot").key(),
            root.fork(StableHash::of_bytes(b"loot").raw()).key()
        );
    }
}
