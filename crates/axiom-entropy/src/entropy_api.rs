//! [`EntropyApi`] — the deterministic entropy facade.
//!
//! [`Self::stream`] expands one root `seed` into an independent stream per
//! `(seed, address, version)`: it digests the space [`Address`] (via
//! [`axiom_space::SpaceApi`]), folds `(seed, digest, version)` into a derived key
//! with the kernel [`axiom_kernel::StableHash`], and seeds an [`EntropyStream`]
//! with it. The same tuple always yields the same stream; distinct sites and
//! distinct versions yield independent, non-overlapping streams. Branchless.

use axiom_kernel::StableHash;
use axiom_space::{Address, SpaceApi};

use crate::entropy_stream::EntropyStream;

/// The deterministic entropy facade. Stateless: a stream is a pure function of
/// `(seed, address, version)`.
#[derive(Debug)]
pub struct EntropyApi;

impl EntropyApi {
    /// The entropy stream for a site: keyed by the root `seed`, the `address`
    /// identity, and the generator `version`. Bumping `version` re-keys the
    /// stream (a versioned behavior change); restoring it restores the stream.
    pub fn stream(seed: u64, address: &Address, version: u32) -> EntropyStream {
        let key =
            StableHash::of_words(&[seed, SpaceApi::digest(address).raw(), u64::from(version)])
                .raw();
        EntropyStream::from_key(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn addr(segments: &[u64]) -> Address {
        segments
            .iter()
            .fold(SpaceApi::root(), |a, &s| SpaceApi::child(&a, s))
    }

    fn first_n(seed: u64, address: &Address, version: u32, n: usize) -> Vec<u64> {
        let mut s = EntropyApi::stream(seed, address, version);
        (0..n).map(|_| s.next_u64()).collect()
    }

    #[test]
    fn same_tuple_is_reproducible() {
        let a = addr(&[1, 2]);
        assert_eq!(first_n(7, &a, 3, 8), first_n(7, &a, 3, 8));
    }

    #[test]
    fn distinct_address_seed_or_version_are_independent() {
        let base = first_n(7, &addr(&[1, 2]), 3, 8);
        assert_ne!(base, first_n(7, &addr(&[1, 3]), 3, 8));
        assert_ne!(base, first_n(8, &addr(&[1, 2]), 3, 8));
        assert_ne!(base, first_n(7, &addr(&[1, 2]), 4, 8));
    }

    #[test]
    fn version_bump_changes_then_restores_the_stream() {
        let a = addr(&[5]);
        let v3 = first_n(1, &a, 3, 8);
        assert_ne!(v3, first_n(1, &a, 4, 8));
        assert_eq!(v3, first_n(1, &a, 3, 8));
    }

    #[test]
    fn golden_first_value_is_stable() {
        // A pinned value (captured from the keying derivation): any change to how
        // (seed, address-digest, version) is folded into the key is caught here.
        assert_eq!(
            EntropyApi::stream(7, &addr(&[1, 2]), 3).next_u64(),
            1_313_699_445_152_985_842
        );
    }

    #[test]
    fn golden_unit_first_value_is_stable() {
        // Pinned first `unit()` draw from the same (seed, address, version) tuple as
        // `golden_first_value_is_stable`: any change to the unit-narrowing of a draw
        // (the `>> 40` over `2^24`) is caught here.
        assert_eq!(
            EntropyApi::stream(7, &addr(&[1, 2]), 3).unit().get(),
            0.071_215_75
        );
    }

    #[test]
    fn golden_int_first_value_is_stable() {
        // Pinned first `int(1000)` draw from the same tuple: any change to the bounded
        // reduction is caught here.
        assert_eq!(EntropyApi::stream(7, &addr(&[1, 2]), 3).int(1000), 71);
    }

    #[test]
    fn golden_weighted_index_first_value_is_stable() {
        // Pinned first `weighted_index([1, 2, 3, 4])` draw from the same tuple: any
        // change to the cumulative-weight selection is caught here.
        assert_eq!(
            EntropyApi::stream(7, &addr(&[1, 2]), 3).weighted_index(&[1, 2, 3, 4]),
            0
        );
    }

    #[test]
    fn golden_shuffle_ordering_is_stable() {
        // Pinned shuffled ordering of `0..8` from the same tuple: any change to the
        // Fisher-Yates draw sequence is caught here.
        let mut items: Vec<u32> = (0..8).collect();
        EntropyApi::stream(7, &addr(&[1, 2]), 3).shuffle(&mut items);
        assert_eq!(items, vec![4, 7, 3, 1, 6, 2, 5, 0]);
    }

    #[test]
    fn keying_is_collision_free_over_a_swept_domain() {
        let mut keys = HashSet::new();
        for seed in 0..5u64 {
            for x in 0..5u64 {
                for version in 0..5u32 {
                    keys.insert(EntropyApi::stream(seed, &addr(&[x]), version).key());
                }
            }
        }
        assert_eq!(keys.len(), 125);
    }
}
