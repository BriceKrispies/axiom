//! The stable identity of one declared state.

use axiom_kernel::StableHash;

/// A stable, declared state identity: the digest of a declared path string.
///
/// The identity is a pure function of the path the author declared — nothing
/// else. It is deliberately **not** derived from a memory address, from
/// declaration or insertion order, from a randomized hash, from Rust's `TypeId`
/// (which is not stable across compiler versions or build sessions), or from
/// global registration order. That is what makes it safe to put in a serialized
/// snapshot, a golden artifact, a diff, a replay, or a migration.
///
/// Being a 64-bit digest, distinct paths can in principle collide.
/// [`crate::StateSchema`] rejects a collision at construction as a deterministic
/// error naming both paths, so a collision is a loud, reproducible failure
/// rather than silent state corruption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateId(u64);

impl StateId {
    /// The reserved null identity, used where "no particular state" is meant.
    pub const NULL: StateId = StateId(0);

    /// The identity of a declared path, e.g. `"puzzle/tick"`.
    pub fn of_path(path: &str) -> Self {
        StateId(StableHash::of_bytes(path.as_bytes()).raw())
    }

    /// Rebuild an identity from its raw digest (decoding a stored snapshot).
    pub const fn from_raw(raw: u64) -> Self {
        StateId(raw)
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
    fn the_same_path_always_digests_the_same_way() {
        assert_eq!(StateId::of_path("puzzle/tick"), StateId::of_path("puzzle/tick"));
    }

    #[test]
    fn different_paths_digest_differently() {
        assert_ne!(StateId::of_path("puzzle/tick"), StateId::of_path("puzzle/solved"));
    }

    #[test]
    fn identity_round_trips_through_its_raw_digest() {
        let id = StateId::of_path("puzzle/ghosts");
        assert_eq!(StateId::from_raw(id.raw()), id);
    }

    #[test]
    fn the_null_identity_is_zero_and_matches_no_real_path() {
        assert_eq!(StateId::NULL.raw(), 0);
        assert_ne!(StateId::of_path("puzzle/tick"), StateId::NULL);
    }

    #[test]
    fn identities_are_ordered_so_iteration_can_be_deterministic() {
        let low = StateId::from_raw(1);
        let high = StateId::from_raw(2);
        assert!(low < high);
    }
}
