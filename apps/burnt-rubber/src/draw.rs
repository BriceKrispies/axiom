//! Drawing deterministic values from the kernel's seeded generator.
//!
//! [`axiom_kernel::DeterministicRng`] is the engine's sanctioned reproducible
//! source: seeded splitmix64, no clock, no OS entropy, identical on every
//! platform. It hands out `u64`s. Everything Burnt Rubber generates — bends,
//! hills, prop placement, traffic — wants floats in a range, signs, and picks
//! from a table instead.
//!
//! [`Draw`] is that thin conversion and **nothing else**. It adds no randomness
//! of its own, holds no global state, and is always constructed from an explicit
//! seed, so "the same seed produces the same course" is true by construction
//! rather than by discipline. Forking (`Draw::fork`) derives an independent
//! sub-stream by salting the seed, which is how scenery for chunk 40 is
//! generated identically whether the player arrived there in the first minute or
//! the third.

use axiom_kernel::DeterministicRng;

/// A seeded drawer of the value shapes this app generates from.
#[derive(Debug, Clone)]
pub struct Draw {
    rng: DeterministicRng,
    seed: u64,
}

impl Draw {
    /// A drawer seeded with `seed`. Two drawers built from the same seed emit
    /// byte-identical sequences.
    pub fn seeded(seed: u64) -> Self {
        Draw {
            rng: DeterministicRng::seeded(seed),
            seed,
        }
    }

    /// The seed this drawer was built from.
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    /// An independent drawer derived from this one's seed and `salt`. The result
    /// depends only on `(seed, salt)`, never on how far this drawer has been
    /// advanced — so a caller can re-derive a specific sub-stream at any time.
    pub fn fork(&self, salt: u64) -> Draw {
        // splitmix64's own mixing function applied to (seed XOR a stirred salt):
        // a cheap avalanche so adjacent salts give unrelated streams.
        let mut z = self
            .seed
            .wrapping_add(salt.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Draw::seeded(z ^ (z >> 31))
    }

    /// The next raw value — the primitive everything else is built from.
    pub fn next_u64(&mut self) -> u64 {
        self.rng.next_u64()
    }

    /// A float uniformly in `[0, 1)`.
    pub fn unit(&mut self) -> f32 {
        // 24 bits is exactly the f32 mantissa, so every draw is representable
        // and the mapping is uniform without rounding surprises.
        (self.rng.next_u64() >> 40) as f32 / (1u32 << 24) as f32
    }

    /// A float uniformly in `[-1, 1)`.
    pub fn signed_unit(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    /// A float uniformly in `[lo, hi)`. A reversed or empty range yields `lo`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.unit() * (hi - lo).max(0.0)
    }

    /// An integer uniformly in the **inclusive** range `[lo, hi]`. A reversed
    /// range yields `lo`.
    pub fn range_u32(&mut self, lo: u32, hi: u32) -> u32 {
        let span = hi.saturating_sub(lo).saturating_add(1);
        lo + self.rng.next_bounded(span.max(1) as u64) as u32
    }

    /// `+1.0` or `-1.0`, evenly.
    pub fn sign(&mut self) -> f32 {
        (self.rng.next_bounded(2) as f32) * 2.0 - 1.0
    }

    /// `true` with probability `p` (clamped to `0..1`).
    pub fn chance(&mut self, p: f32) -> bool {
        self.unit() < p.clamp(0.0, 1.0)
    }

    /// An index uniformly in `[0, len)`. A `len` of `0` yields `0`.
    pub fn index(&mut self, len: usize) -> usize {
        self.rng.next_bounded(len.max(1) as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_draws_the_same_sequence() {
        let take = |seed| {
            let mut d = Draw::seeded(seed);
            (0..64).map(|_| d.next_u64()).collect::<Vec<_>>()
        };
        assert_eq!(take(7), take(7));
        assert_ne!(take(7), take(8));
    }

    #[test]
    fn units_stay_in_their_stated_ranges() {
        let mut d = Draw::seeded(0xC0FFEE);
        for _ in 0..4096 {
            let u = d.unit();
            assert!((0.0..1.0).contains(&u), "unit out of range: {u}");
            let s = d.signed_unit();
            assert!((-1.0..1.0).contains(&s), "signed unit out of range: {s}");
            let r = d.range(-3.0, 9.0);
            assert!((-3.0..9.0).contains(&r), "range out of range: {r}");
            let n = d.range_u32(4, 9);
            assert!((4..=9).contains(&n), "int out of range: {n}");
            assert!(d.sign().abs() == 1.0);
            assert!(d.index(5) < 5);
        }
    }

    #[test]
    fn degenerate_ranges_collapse_to_their_low_bound() {
        let mut d = Draw::seeded(1);
        assert_eq!(d.range(4.0, 4.0), 4.0);
        assert_eq!(d.range(9.0, 2.0), 9.0);
        assert_eq!(d.range_u32(6, 6), 6);
        assert_eq!(d.range_u32(9, 2), 9);
        assert_eq!(d.index(0), 0);
        assert!(!d.chance(0.0));
        assert!(d.chance(1.0));
        assert!(!d.chance(-1.0));
        assert!(d.chance(2.0));
    }

    /// Forking is a pure function of `(seed, salt)`: it must not depend on how
    /// far the parent has been advanced, or a chunk's scenery would change
    /// depending on when the player reached it.
    #[test]
    fn forks_depend_only_on_seed_and_salt() {
        let parent = Draw::seeded(1234);
        let mut advanced = parent.clone();
        for _ in 0..500 {
            advanced.next_u64();
        }
        let a: Vec<u64> = {
            let mut f = parent.fork(17);
            (0..8).map(|_| f.next_u64()).collect()
        };
        let b: Vec<u64> = {
            let mut f = advanced.fork(17);
            (0..8).map(|_| f.next_u64()).collect()
        };
        assert_eq!(a, b, "a fork ignores the parent's position");

        let c: Vec<u64> = {
            let mut f = parent.fork(18);
            (0..8).map(|_| f.next_u64()).collect()
        };
        assert_ne!(a, c, "adjacent salts give unrelated streams");
        assert_eq!(parent.seed(), 1234);
    }

    /// A rough distribution check: `sign` and `chance` must not be lopsided, or
    /// every bend in the course would turn the same way.
    #[test]
    fn signs_and_chances_are_balanced() {
        let mut d = Draw::seeded(0xABCDEF);
        let positive = (0..8192).filter(|_| d.sign() > 0.0).count();
        assert!(
            (3600..4600).contains(&positive),
            "signs are lopsided: {positive}/8192"
        );
        let hits = (0..8192).filter(|_| d.chance(0.25)).count();
        assert!((1800..2300).contains(&hits), "chance is skewed: {hits}/8192");
    }
}
