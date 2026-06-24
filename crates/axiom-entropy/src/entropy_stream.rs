//! [`EntropyStream`] — one keyed, reproducible entropy stream.
//!
//! A stream carries a stable **key** (its identity, independent of how far it has
//! been drawn) and a live [`axiom_kernel::DeterministicRng`] seeded from that key.
//! Two streams with the same key produce byte-identical sequences; [`Self::fork`]
//! derives an isolated sub-stream from the stable key (not the draw position), so
//! a fork is reproducible no matter how far the parent has advanced. It invents no
//! randomness — it routes the kernel's deterministic source.

use axiom_kernel::{DeterministicRng, StableHash};

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
}
