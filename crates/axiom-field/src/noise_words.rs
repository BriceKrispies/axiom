//! The wire encoding of the [`crate::FieldOp::Noise`] and [`crate::FieldOp::Fbm`]
//! parameter words — stated **once**, here.
//!
//! Three places must agree on this encoding: the signature table (which declares
//! how many words the operators carry), the authoring surface (which writes
//! them), and the evaluator (which reads them). Two spellings of one format are
//! two ways for an authored graph and an evaluated one to disagree, so the
//! encoder and the decoder live side by side and round-trip in a test.
//!
//! ## Why the knob count is pinned here rather than derived from a type's size
//!
//! [`FBM_KNOB_WORDS`] used to be `size_of::<FbmConfig>() / size_of::<u32>()`.
//! That is a **memory-layout coincidence** standing in for a semantic parameter
//! count: it happened to equal four, and it would keep happening to equal
//! something after a knob was added, removed, or repadded — silently changing the
//! operator's arity and every graph's bytes.
//!
//! The count is now pinned by [`fbm_words`], which **destructures an
//! [`FbmConfig`] exhaustively** and emits a `[u32; FBM_KNOB_WORDS]`. Adding a
//! knob to `FbmConfig` makes that pattern fail to compile (`E0027`), and widening
//! the array to carry it fails to compile against the constant — so the arity can
//! only change by a deliberate edit here, never by a layout accident.

use core::mem::size_of;

use axiom_kernel::Ratio;
use axiom_noise::{FbmConfig, Frequency, Lacunarity};
use axiom_recipe::Param;

/// The two 32-bit words a `u64` seed occupies. This one *is* the definition of
/// splitting a `u64` into `u32` words, not a coincidence: word 0 is the low half
/// and word 1 the high half, matching the crate's little-endian byte order.
pub(crate) const SEED_WORDS: usize = size_of::<u64>() / size_of::<u32>();

/// The knob words an `Fbm` node carries after its seed: `octaves`, `frequency`,
/// `lacunarity`, `gain` — one 32-bit word each, in that order. Pinned by
/// [`fbm_words`] and [`fbm_config`], which name every knob explicitly.
pub(crate) const FBM_KNOB_WORDS: usize = 4;

/// The two words of `seed`, low half first.
pub(crate) fn seed_words(seed: u64) -> [u32; SEED_WORDS] {
    [seed as u32, (seed >> 32) as u32]
}

/// The knob words of `config`, in wire order.
///
/// The exhaustive destructuring is the point: it names every knob
/// [`FbmConfig`] has, so a new knob is a compile error here rather than a silent
/// change to [`FBM_KNOB_WORDS`].
pub(crate) fn fbm_words(config: FbmConfig) -> [u32; FBM_KNOB_WORDS] {
    let FbmConfig {
        octaves,
        frequency,
        lacunarity,
        gain,
    } = config;
    [
        octaves,
        frequency.get().to_bits(),
        lacunarity.get().to_bits(),
        gain.get().to_bits(),
    ]
}

/// The `u64` seed a node's first two parameter words carry.
pub(crate) fn seed(words: &[Param]) -> u64 {
    u64::from(word(words, 0)) | (u64::from(word(words, 1)) << 32)
}

/// The [`FbmConfig`] a node's knob words carry.
///
/// **Total**, because the words are raw bits nothing has proved finite: a
/// frequency or gain word that decodes to NaN or an infinity reads as `0.0`
/// (the kernel's `finite_or_zero` rule), and a non-finite lacunarity reads as
/// [`Lacunarity::DOUBLING`], the canonical octave-doubling default. Naming every
/// knob in the struct literal is what pins [`FBM_KNOB_WORDS`].
pub(crate) fn fbm_config(words: &[Param]) -> FbmConfig {
    FbmConfig {
        octaves: word(words, SEED_WORDS),
        frequency: Frequency::finite_or_zero(f32::from_bits(word(words, SEED_WORDS + 1))),
        lacunarity: Lacunarity::new(f32::from_bits(word(words, SEED_WORDS + 2)))
            .map_or(Lacunarity::DOUBLING, |value| value),
        gain: Ratio::finite_or_zero(f32::from_bits(word(words, SEED_WORDS + 3))),
    }
}

/// Parameter word `slot`, or `0` when the node carries no such word.
fn word(words: &[Param], slot: usize) -> u32 {
    words.get(slot).map_or(0, |param| param.bits())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(words: &[u32]) -> Vec<Param> {
        words.iter().copied().map(Param::from_bits).collect()
    }

    fn config() -> FbmConfig {
        FbmConfig {
            gain: Ratio::finite_or_zero(0.375),
            lacunarity: Lacunarity::new(2.25).expect("2.25 is finite"),
            ..FbmConfig::new(5, Frequency::finite_or_zero(1.75))
        }
    }

    #[test]
    fn the_arities_are_the_ones_the_signature_table_declares() {
        assert_eq!(SEED_WORDS, 2);
        assert_eq!(FBM_KNOB_WORDS, 4);
        assert_eq!(seed_words(0).len(), SEED_WORDS);
        assert_eq!(fbm_words(config()).len(), FBM_KNOB_WORDS);
    }

    #[test]
    fn a_seed_round_trips_through_its_two_words_low_half_first() {
        let value = 0x0123_4567_89AB_CDEF_u64;
        assert_eq!(seed_words(value), [0x89AB_CDEF, 0x0123_4567]);
        assert_eq!(seed(&params(&seed_words(value))), value);
        assert_eq!(seed(&[]), 0);
    }

    #[test]
    fn a_config_round_trips_through_its_knob_words() {
        let mut words = seed_words(7).to_vec();
        words.extend(fbm_words(config()));
        let decoded = fbm_config(&params(&words));
        assert_eq!(decoded, config());
        assert_eq!(seed(&params(&words)), 7);
    }

    #[test]
    fn every_knob_word_reaches_exactly_one_knob() {
        let base: Vec<u32> = seed_words(0)
            .iter()
            .copied()
            .chain(fbm_words(config()))
            .collect();
        let moved = |index: usize, bits: u32| {
            let mut words = base.clone();
            words[SEED_WORDS + index] = bits;
            fbm_config(&params(&words))
        };
        assert_eq!(moved(0, 9).octaves, 9);
        assert_eq!(moved(1, 0.5_f32.to_bits()).frequency.get(), 0.5);
        assert_eq!(moved(2, 3.0_f32.to_bits()).lacunarity.get(), 3.0);
        assert_eq!(moved(3, 0.25_f32.to_bits()).gain.get(), 0.25);
    }

    #[test]
    fn a_non_finite_knob_word_decodes_to_its_documented_fallback() {
        let hostile: Vec<u32> = seed_words(0)
            .iter()
            .copied()
            .chain([1, f32::NAN.to_bits(), f32::INFINITY.to_bits(), f32::NAN.to_bits()])
            .collect();
        let decoded = fbm_config(&params(&hostile));
        assert_eq!(decoded.frequency.get(), 0.0);
        assert_eq!(decoded.lacunarity, Lacunarity::DOUBLING);
        assert_eq!(decoded.gain.get(), 0.0);
        assert_eq!(decoded.octaves, 1);
    }

    #[test]
    fn a_node_missing_its_knob_words_decodes_to_zeroes() {
        let decoded = fbm_config(&[]);
        assert_eq!(decoded.octaves, 0);
        assert_eq!(decoded.frequency.get(), 0.0);
        assert_eq!(decoded.gain.get(), 0.0);
        // Bit pattern 0 is `+0.0`, which is finite, so the lacunarity fallback
        // does not fire — a missing word reads as a zero lacunarity.
        assert_eq!(decoded.lacunarity.get(), 0.0);
    }
}
