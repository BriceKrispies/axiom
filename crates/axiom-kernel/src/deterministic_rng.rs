//! A seeded, fully deterministic pseudo-random generator.

use crate::binary_reader::BinaryReader;
use crate::binary_writer::BinaryWriter;
use crate::reflect::Reflect;
use crate::result::KernelResult;
use crate::type_schema::{FieldSchema, TypeSchema};

/// A deterministic pseudo-random number generator.
/// This is a *seeded* generator: its entire output is a pure function of the
/// seed it was constructed with, so the same seed always yields the same
/// sequence on every platform. It reads no entropy, no clock and no global
/// state — it is the kernel's sanctioned deterministic "random source", suitable
/// for replayable simulation, fuzzing and adversarial-network models where the
/// sequence must be reproducible.
/// The core step is `splitmix64`: cheap, branchless, and well-distributed.
#[derive(Debug, Clone)]
pub struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    /// Construct a generator from a seed. Two generators built from the same
    /// seed produce byte-identical sequences.
    pub const fn seeded(seed: u64) -> Self {
        DeterministicRng { state: seed }
    }

    /// The generator's current internal state — the single value that fully
    /// determines its future sequence. Capturing this (and restoring it via
    /// [`Self::from_state`]) snapshots an in-flight generator so a restored
    /// session continues the *identical* sequence.
    pub const fn state(&self) -> u64 {
        self.state
    }

    /// Reconstruct a generator at an exact internal state captured by
    /// [`Self::state`]. Unlike [`Self::seeded`] (which begins a fresh sequence
    /// from a seed), this resumes an in-flight sequence mid-stream.
    pub const fn from_state(state: u64) -> Self {
        DeterministicRng { state }
    }

    /// Advance the generator and return the next 64-bit value (`splitmix64`).
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value uniformly in `[0, bound)`, via Lemire's multiply-high reduction
    /// (branchless, negligible bias, no division). A `bound` of `0` yields `0`.
    pub fn next_bounded(&mut self, bound: u64) -> u64 {
        let wide = (self.next_u64() as u128) * (bound as u128);
        (wide >> 64) as u64
    }

    /// A `bool` that is `true` with probability `per_mille / 1000`. `0` is always
    /// `false`; `1000` (or more) is always `true`. Useful for deterministic
    /// drop/delay decisions in a modelled network.
    pub fn next_bool_in_thousand(&mut self, per_mille: u32) -> bool {
        self.next_bounded(1000) < per_mille as u64
    }
}

/// The generator round-trips through its single `u64` of state, so a snapshot can
/// embed a live RNG and a restore resumes the identical sequence — the capability
/// any game with randomness (loot, spawns, shuffles, crits) needs to replay,
/// rewind, or recover correctly.
impl Reflect for DeterministicRng {
    const SCHEMA: TypeSchema =
        TypeSchema::new("DeterministicRng", &[FieldSchema::new("state", "u64")]);

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.state.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        u64::reflect_read(reader).map(DeterministicRng::from_state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_yields_identical_sequence() {
        let mut a = DeterministicRng::seeded(0xABCD_1234);
        let mut b = DeterministicRng::seeded(0xABCD_1234);
        for _ in 0..64 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = DeterministicRng::seeded(1);
        let mut b = DeterministicRng::seeded(2);
        // Overwhelmingly likely to differ within a few draws; assert across many.
        let differ = (0..16).any(|_| a.next_u64() != b.next_u64());
        assert!(differ, "distinct seeds must produce distinct sequences");
    }

    #[test]
    fn next_bounded_stays_in_range_and_covers_bound_one() {
        let mut rng = DeterministicRng::seeded(42);
        for _ in 0..1000 {
            assert!(rng.next_bounded(6) < 6);
        }
        // bound == 1 is always 0; bound == 0 is defined as 0.
        assert_eq!(rng.next_bounded(1), 0);
        assert_eq!(rng.next_bounded(0), 0);
    }

    #[test]
    fn next_bounded_actually_varies() {
        // Kills a `next_bounded -> 0` mutant: a d20 must yield at least two
        // distinct faces across many rolls.
        let mut rng = DeterministicRng::seeded(7);
        let first = rng.next_bounded(20);
        let varied = (0..200).any(|_| rng.next_bounded(20) != first);
        assert!(varied);
    }

    #[test]
    fn next_bool_in_thousand_endpoints_are_total() {
        let mut rng = DeterministicRng::seeded(99);
        for _ in 0..100 {
            assert!(!rng.next_bool_in_thousand(0), "0 per-mille is never true");
            assert!(
                rng.next_bool_in_thousand(1000),
                "1000 per-mille is always true"
            );
        }
    }

    #[test]
    fn state_and_from_state_capture_the_exact_position() {
        let mut rng = DeterministicRng::seeded(0xFEED);
        (0..5).for_each(|_| {
            rng.next_u64();
        });
        let mut resumed = DeterministicRng::from_state(rng.state());
        assert_eq!(resumed.state(), rng.state());
        assert_eq!(resumed.next_u64(), {
            let mut original = DeterministicRng::from_state(rng.state());
            original.next_u64()
        });
    }

    #[test]
    fn reflect_round_trips_the_state() {
        let mut rng = DeterministicRng::seeded(0x1234_5678);
        (0..3).for_each(|_| {
            rng.next_u64();
        });
        let mut w = BinaryWriter::new();
        rng.reflect_write(&mut w);
        let bytes = w.into_bytes();
        let restored = DeterministicRng::reflect_read(&mut BinaryReader::new(&bytes)).unwrap();
        assert_eq!(restored.state(), rng.state());
    }

    #[test]
    fn a_restored_generator_continues_the_identical_sequence() {
        let mut original = DeterministicRng::seeded(0xABCD_EF01);
        (0..8).for_each(|_| {
            original.next_u64();
        });
        let mut w = BinaryWriter::new();
        original.reflect_write(&mut w);
        let mut restored =
            DeterministicRng::reflect_read(&mut BinaryReader::new(&w.into_bytes())).unwrap();
        let continuation: Vec<u64> = (0..16).map(|_| original.next_u64()).collect();
        let replayed: Vec<u64> = (0..16).map(|_| restored.next_u64()).collect();
        assert_eq!(continuation, replayed);
    }

    #[test]
    fn reflect_read_propagates_truncation() {
        assert!(DeterministicRng::reflect_read(&mut BinaryReader::new(&[])).is_err());
    }

    #[test]
    fn reflect_schema_names_its_state_field() {
        assert_eq!(<DeterministicRng as Reflect>::SCHEMA.name(), "DeterministicRng");
        assert_eq!(
            <DeterministicRng as Reflect>::SCHEMA.fields()[0].name(),
            "state"
        );
    }

    #[test]
    fn next_bool_in_thousand_can_be_both() {
        let mut rng = DeterministicRng::seeded(123);
        let mut saw_true = false;
        let mut saw_false = false;
        for _ in 0..200 {
            if rng.next_bool_in_thousand(500) {
                saw_true = true;
            } else {
                saw_false = true;
            }
        }
        assert!(saw_true && saw_false);
    }
}
