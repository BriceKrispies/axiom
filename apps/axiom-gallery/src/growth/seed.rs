//! World seed: the deterministic root for all proc-gen. Audit: worldgen.md §2.
/// Audit: form keys seed/height_scale/octaves/frequency.
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
    /// Audit: form keys seed/height_scale/octaves/frequency.
    ///
    /// NOTE: an empty `seed_str` hashes to the FNV offset basis; callers that
    /// want a fresh random world per run should detect the empty string and
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
        // The documented empty-seed rule: "" hashes to the FNV-1a offset basis
        // (an empty byte fold), which callers detect to substitute a random world.
        // Swapping in the kernel `StableHash` must preserve this exact value.
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
        // The root value path is shared, so form and str-seed agree on `value`.
        for s in ["", "a", "longer seed phrase", "12345"] {
            assert_eq!(
                WorldSeed::from_form(s, 1.0, 5, 1.5).value,
                WorldSeed::from_str_seed(s).value
            );
        }
    }
}
