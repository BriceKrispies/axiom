//! World seed: the deterministic root for all proc-gen.
use axiom_entropy::{EntropyApi, EntropyStream};
use axiom_space::{Address, SpaceApi};

/// Opaque, fixed address segment naming the growth worldgen root site — a depth-1
/// child of the space root, so the entropy key derived from `(seed, address,
/// version)` is reproducible across runs and platforms. Kept byte-identical to
/// `axiom_planetgen`'s own root segment so the app's genome / vista streams and
/// the graduated pipeline's stage streams share one keying convention.
const WORLDGEN_ROOT_SEGMENT: u64 = 0x_67_72_6F_77_74_68_00_01; // "growth\0\x01"
/// Generator version for the worldgen entropy key.
const WORLDGEN_VERSION: u32 = 1;

/// The deterministic worldgen root [`EntropyStream`] for a `u64` seed. The genome
/// sampler (`presets`) draws sequentially off it and the scenic composers
/// (`vista`) fork isolated sub-streams — independent of the planet-generation
/// stages, which mint their own equivalent stream inside `axiom_planetgen`.
pub fn worldgen_stream(seed: u64) -> EntropyStream {
    let address: Address = SpaceApi::child(&SpaceApi::root(), WORLDGEN_ROOT_SEGMENT);
    EntropyApi::stream(seed, &address, WORLDGEN_VERSION)
}

#[derive(Debug, Clone)]
pub struct WorldSeed {
    pub value: u64,
    pub height_scale: f32,
    pub octaves: u32,
    pub frequency: f32,
}

impl WorldSeed {
    pub fn from_value(value: u64) -> Self {
        Self {
            value,
            height_scale: 1.0,
            octaves: 5,
            frequency: 1.5,
        }
    }

    /// Hash a string seed deterministically (FNV-1a). Empty → 0 (caller randomises).
    pub fn from_str_seed(s: &str) -> Self {
        Self::from_value(fnv1a(s))
    }

    /// Build a seed from the full world-gen form: the seed string is hashed
    /// (FNV-1a) into the root value, and the noise knobs are carried through.
    ///
    /// An empty `seed_str` hashes to the FNV offset basis; callers that want a
    /// fresh random world per run should detect the empty string and
    /// substitute a randomised seed before calling generation.
    pub fn from_form(seed_str: &str, height_scale: f32, octaves: u32, frequency: f32) -> Self {
        Self {
            value: fnv1a(seed_str),
            height_scale,
            octaves,
            frequency,
        }
    }
}

/// FNV-1a 64-bit hash via the kernel's platform-stable [`axiom_kernel::StableHash`]
/// instead of a hand-rolled copy. Stable across runs/platforms; same string → same
/// value. An empty string hashes to the FNV offset basis
/// (`StableHash::of_bytes(&[])`), preserving the documented empty-seed behavior.
fn fnv1a(s: &str) -> u64 {
    axiom_kernel::StableHash::of_bytes(s.as_bytes()).raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_seed_is_stable() {
        let a = WorldSeed::from_str_seed("growth");
        let b = WorldSeed::from_str_seed("growth");
        assert_eq!(a.value, b.value);
    }

    #[test]
    fn different_strings_differ() {
        let a = WorldSeed::from_str_seed("growth");
        let b = WorldSeed::from_str_seed("Growth");
        let c = WorldSeed::from_str_seed("growth ");
        assert_ne!(a.value, b.value);
        assert_ne!(a.value, c.value);
        assert_ne!(b.value, c.value);
    }

    #[test]
    fn empty_seed_hashes_to_the_fnv_offset_basis() {
        assert_eq!(WorldSeed::from_str_seed("").value, 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn from_form_carries_noise_knobs() {
        let s = WorldSeed::from_form("seedstr", 2.5, 7, 0.8);
        assert_eq!(s.value, WorldSeed::from_str_seed("seedstr").value);
        assert_eq!(s.height_scale, 2.5);
        assert_eq!(s.octaves, 7);
        assert_eq!(s.frequency, 0.8);
    }

    #[test]
    fn from_form_matches_str_seed_value() {
        for s in ["", "a", "longer seed phrase", "12345"] {
            assert_eq!(
                WorldSeed::from_form(s, 1.0, 5, 1.5).value,
                WorldSeed::from_str_seed(s).value
            );
        }
    }
}
