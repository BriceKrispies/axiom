//! `RngHub`: the deterministic RNG seam (SPEC-01) the TS `Rng` projection drives.
//!
//! This is the native, fully-testable core behind the `NativeBridge`'s
//! `rngUnit`/`rngBelow`/`rngWeighted`/`rngPermutation`/`rngStream` methods. The
//! decisions live native-side, exactly as the contract demands: the draw sequence
//! and the named sub-stream identity are owned here, and the TS projection only
//! reshapes the primitive results into the author surface (it never re-decides
//! them). The browser binds these through the `wasm32` [`crate::wasm`] boundary;
//! native slice tests drive this struct directly.
//!
//! It is a pure router over the real [`axiom_entropy`] facade — the engine's one
//! deterministic entropy primitive — not a bespoke RNG: the session seed plus the
//! root [`axiom_space::Address`] keys the root [`EntropyStream`], and every
//! `rngStream(parent, name)` resolves a reproducible named sub-stream of its
//! parent (`EntropyStream::named`). Resolving the same `(parent, name)` twice
//! returns the same stream id, so a stream's draw position advances exactly once
//! per logical stream — the property an author and a replay both rely on.
//!
//! As app code this is outside the engine's branchless / coverage gates, but the
//! determinism it carries is the same invariant the spine holds: the same seed
//! and the same call sequence produce a byte-identical draw sequence on every run.

use std::collections::HashMap;

use axiom_entropy::{EntropyApi, EntropyStream};
use axiom_space::SpaceApi;

/// A table of deterministic entropy streams, keyed by the session seed. Stream id
/// `0` is the root; `rngStream(parent, name)` mints (or resolves) a named child
/// and hands back its id. JS holds these ids as opaque numbers.
#[derive(Debug)]
pub struct RngHub {
    /// The live streams, indexed by the id JS holds (`0` is the root).
    streams: Vec<EntropyStream>,
    /// Resolves a `(parent id, name)` pair to the single stream id it owns, so a
    /// re-resolution of the same named sub-stream returns the same id (and thus
    /// the same advancing draw position) rather than forking a fresh one.
    by_name: HashMap<(u32, String), u32>,
}

impl RngHub {
    /// Build the hub for a session `seed`: the root stream is keyed by
    /// `(seed, root address, version 0)` through the real entropy facade, so two
    /// sessions with the same seed draw byte-identical sequences.
    pub fn new(seed: u64) -> Self {
        let root = EntropyApi::stream(seed, &SpaceApi::root(), 0);
        RngHub {
            streams: vec![root],
            by_name: HashMap::new(),
        }
    }

    /// Resolve the id of the named sub-stream of `parent` (`Rng::stream`,
    /// SPEC-01). Idempotent: the same `(parent, name)` always resolves to the same
    /// id. A stale `parent` id is a clean no-op resolving to the root (`0`).
    pub fn stream(&mut self, parent: u32, name: &str) -> u32 {
        let key = (parent, name.to_string());
        // An already-resolved (parent, name) returns its id; otherwise mint a
        // child iff the parent id is live, else the neutral root id `0`.
        let existing = self.by_name.get(&key).copied();
        existing.unwrap_or_else(|| {
            let child = self.streams.get(parent as usize).map(|p| p.named(name));
            child
                .map(|c| {
                    let id = self.streams.len() as u32;
                    self.streams.push(c);
                    self.by_name.insert(key, id);
                    id
                })
                .unwrap_or(0)
        })
    }

    /// A uniform draw in `[0, 1)` from `stream` (`Rng::unit`). A stale id draws
    /// the neutral `0.0`.
    pub fn unit(&mut self, stream: u32) -> f64 {
        self.streams
            .get_mut(stream as usize)
            .map_or(0.0, |s| f64::from(s.unit().get()))
    }

    /// A uniform integer in `[0, max_exclusive)` from `stream` (`Rng::int`). A
    /// stale id draws the neutral `0`.
    pub fn below(&mut self, stream: u32, max_exclusive: u64) -> u64 {
        self.streams
            .get_mut(stream as usize)
            .map_or(0, |s| s.int(max_exclusive))
    }

    /// The index `weights` selects, drawn proportionally to the integer weights,
    /// from `stream` (`Rng::weighted`). A stale id selects the neutral `0`.
    pub fn weighted(&mut self, stream: u32, weights: &[u64]) -> u32 {
        self.streams
            .get_mut(stream as usize)
            .map_or(0, |s| s.weighted_index(weights) as u32)
    }

    /// A Fisher-Yates permutation of `[0, length)` drawn from `stream`
    /// (`Rng::permutation`). A stale id yields the empty permutation.
    pub fn permutation(&mut self, stream: u32, length: u32) -> Vec<u32> {
        self.streams
            .get_mut(stream as usize)
            .map(|s| {
                let mut items: Vec<u32> = (0..length).collect();
                s.shuffle(&mut items);
                items
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_reproduces_the_unit_sequence() {
        let draw = |seed| -> Vec<f64> {
            let mut hub = RngHub::new(seed);
            (0..16).map(|_| hub.unit(0)).collect()
        };
        assert_eq!(draw(7), draw(7));
        assert_ne!(draw(7), draw(8));
        assert!(draw(7).iter().all(|&v| (0.0..1.0).contains(&v)));
    }

    #[test]
    fn below_is_bounded_and_reproducible() {
        let mut a = RngHub::new(3);
        let mut b = RngHub::new(3);
        (0..256).for_each(|_| {
            let x = a.below(0, 10);
            assert!(x < 10);
            assert_eq!(x, b.below(0, 10));
        });
    }

    #[test]
    fn named_stream_is_resolved_idempotently_and_advances_once() {
        let mut hub = RngHub::new(11);
        let loot = hub.stream(0, "loot");
        assert_eq!(hub.stream(0, "loot"), loot);
        assert_ne!(loot, 0);
        assert_ne!(hub.stream(0, "spawn"), loot);

        let mixed: Vec<u64> = (0..8).map(|_| hub.below(loot, 1_000_000)).collect();
        let mut fresh = RngHub::new(11);
        let fresh_loot = fresh.stream(0, "loot");
        let direct: Vec<u64> = (0..8).map(|_| fresh.below(fresh_loot, 1_000_000)).collect();
        assert_eq!(mixed, direct);
    }

    #[test]
    fn weighted_honours_the_weights() {
        let mut hub = RngHub::new(17);
        // All weight on index 1 ⇒ always 1; zero-weight buckets never chosen.
        assert!((0..128).all(|_| hub.weighted(0, &[0, 5, 0]) == 1));
    }

    #[test]
    fn permutation_is_a_reproducible_shuffle() {
        let perm = |seed| -> Vec<u32> {
            let mut hub = RngHub::new(seed);
            hub.permutation(0, 32)
        };
        let a = perm(23);
        assert_eq!(a, perm(23));
        let mut sorted = a.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..32).collect::<Vec<u32>>());
    }

    #[test]
    fn stale_ids_are_clean_no_ops() {
        let mut hub = RngHub::new(1);
        // A parent id past the end resolves to the root (0), and every draw on a
        // stale id returns the neutral value rather than panicking.
        assert_eq!(hub.stream(99, "x"), 0);
        assert_eq!(hub.unit(99), 0.0);
        assert_eq!(hub.below(99, 10), 0);
        assert_eq!(hub.weighted(99, &[1, 2, 3]), 0);
        assert!(hub.permutation(99, 4).is_empty());
    }
}
