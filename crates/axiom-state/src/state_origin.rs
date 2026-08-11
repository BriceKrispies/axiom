//! Who wrote a change.

use axiom_kernel::StableHash;

/// A deterministic label for whoever authored a patch.
///
/// Not a registry — the identity is the digest of a name the caller declares, so
/// two processes independently naming `"scoring"` agree without coordinating.
/// Its job is diagnostic: when two patches collide, the conflict names both
/// writers instead of saying only that something clashed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateOrigin(u64);

impl StateOrigin {
    /// The unattributed origin, for callers that do not name themselves.
    pub const ANONYMOUS: StateOrigin = StateOrigin(0);

    /// The origin named `name`.
    pub fn of_name(name: &str) -> Self {
        StateOrigin(StableHash::of_bytes(name.as_bytes()).raw())
    }

    /// Rebuild from a raw digest (decoding a stored patch).
    pub const fn from_raw(raw: u64) -> Self {
        StateOrigin(raw)
    }

    /// The raw 64-bit digest.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_name_always_digests_the_same_way() {
        assert_eq!(StateOrigin::of_name("scoring"), StateOrigin::of_name("scoring"));
    }

    #[test]
    fn different_names_digest_differently() {
        assert_ne!(StateOrigin::of_name("scoring"), StateOrigin::of_name("physics"));
    }

    #[test]
    fn the_anonymous_origin_is_zero_and_matches_no_name() {
        assert_eq!(StateOrigin::ANONYMOUS.raw(), 0);
        assert_ne!(StateOrigin::of_name("scoring"), StateOrigin::ANONYMOUS);
    }

    #[test]
    fn an_origin_round_trips_through_its_raw_digest() {
        let origin = StateOrigin::of_name("scoring");
        assert_eq!(StateOrigin::from_raw(origin.raw()), origin);
    }
}
